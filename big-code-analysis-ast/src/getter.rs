// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

//! The [`Getter`] accessors — names, space kinds, Halstead classes — and
//! their per-language impls.

use crate::token_role::TokenRole;

use crate::space_kind::SpaceKind;
use crate::traits::Search;

use crate::*;

/// Bounds- and UTF-8-checked text extraction for a node's byte span.
///
/// `code` is `&[u8]`, so slicing it carries no char-boundary
/// precondition; the two guards here cover two unrelated failure modes:
///
/// * `std::str::from_utf8` rejects non-UTF-8 bytes. This one is
///   reachable from ordinary use — `big_code_analysis::Ast::parse` accepts
///   arbitrary bytes, so a node span in a partially-binary source need
///   not be valid UTF-8.
/// * `code.get` bounds-checks the range. This one is reachable only by
///   violating the same-parse precondition documented on `Getter`, i.e.
///   `big_code_analysis::Ast::from_tree_sitter` adopting a tree built from longer
///   source than the `code` passed alongside it.
///
/// Both degrade to `None`. The walker stores a space's name as
/// `Option<String>` (`spaces.rs`), so that records an unnamed space
/// rather than crashing — note this is *not* the same path as a node
/// with no `name` field, which `get_func_space_name` reports as
/// `Some("<anonymous>")` without reaching here. The unguarded sibling
/// slice sites return infallible
/// types feeding metric arithmetic or rendered output; there the only
/// available fallback would be a fabricated empty value that silently
/// corrupts a count, so they rely on the precondition instead. The
/// asymmetry is deliberate: guarding is free here because `Option` is
/// already part of this signature's contract (#1059).
#[inline]
fn node_text<'a>(code: &'a [u8], node: &Node) -> Option<&'a str> {
    code.get(node.start_byte()..node.end_byte())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
}

macro_rules! get_operator {
    ($language:ident) => {
        #[inline]
        fn get_operator_id_as_str(id: u16) -> &'static str {
            let typ = id.into();
            match typ {
                $language::LPAREN => "()",
                $language::LBRACK => "[]",
                $language::LBRACE => "{}",
                _ => typ.into(),
            }
        }
    };
}

// Emit a `Getter::get_op_type` body for a JS-family language. The four
// JS-family grammars (JavaScript, MozJS, TypeScript, TSX) share most of
// their Halstead operator/operand kind classifications; per-language
// deltas are passed as bracketed extras so all four impls stay in
// lockstep when a kind is added or removed (issue #299).
//
// `$op_extras` per language:
//   * JavaScript / MozJS: `OptionalChain` — the bare `?.` token (these
//     grammars expose no `optional_chain` wrapper).
//   * TypeScript / TSX:   `QMARKDOT`, `PredefinedType` — `QMARKDOT` is
//     the bare `?.` token under the `optional_chain` wrapper (issue
//     #281); `PredefinedType` is the TS type keyword set (`string`,
//     `number`, `boolean`, …).
//
// `$operand_extras` per language:
//   * JavaScript / MozJS / TSX: `Identifier2`, `String2` — anonymous
//     keyword aliases the JS grammar exposes for `Identifier` and
//     `String`. TSX's `String2` (kind_id 261) is its string-literal
//     alias, so it stays an operand like JS's.
//   * TypeScript: none. TS's own aliases are either operators or
//     deliberately unclassified (below).
//     The `: string` type-keyword aliases (TS `String2`, kind_id 135;
//     TSX `String3`, kind_id 141) are deliberately absent: they are
//     emitted only as the child of a `predefined_type` wrapper, which
//     already contributes the text-keyed `"string"` operator, and
//     #313's listing of the keyword child as an operand counted one
//     source token twice (#1261, which also narrowed
//     `Checker::is_string` to match).
//
// **A member access contributes its leaves, never the composite.**
// `a.b` is the operands `a` and `b` plus the `.` operator — three
// vocabulary entries, not four. Until #1263 the operand arm listed the
// `member_expression` wrapper *and* the walker descended into its
// `object` / `property` children, so every access billed one extra
// `N2` entry keyed on the whole `a.b` text (and `a.b.c` billed two:
// `a.b` and `a.b.c`). The composite is what the rest of the workspace
// already excludes — C, C++, Java, Rust, Python, Go, Kotlin, Ruby, Lua
// and PHP were all measured leaves-only for the identical shape — so
// identical code scored differently by language. The same call applies
// to TS's `nested_identifier` (`namespace N.M`), C#'s
// `qualified_name` / `generic_name` / `alias_qualified_name`, and
// Groovy's `qualified_name` / `qualified_type`.
//
// This is grammar-dispatch section 5's "any predicate listing both a
// container and a kind it can contain double-counts", and its section
// 6 corollary is why `PrivatePropertyIdentifier` joins the operand arm
// in the same change: `this.#x`'s `#x` leaf was in no operand list, so
// the composite had been its only count, and a bare deletion would
// have regressed private-field access to zero operands. (The `#x` in
// the *field definition* `#x = 1` had never been counted at all — no
// wrapper covered it — so this fixes that half too.)
//
// The `TemplateString` interpolation guard is shared verbatim (issue
// #192): a bare `` `...` `` mirrors a `"..."` operand, but an
// interpolated template must yield `Unknown` because its inner
// `TemplateSubstitution` expressions are walked separately.
//
// The `Regex` arms are shared the same way (issue #1314), and for the
// same reason: all four grammars spell a regex literal identically —
// a `regex` wrapper whose two delimiters are `SLASH`, the kind id real
// division uses — so neither arm has a per-language delta to hoist to
// a call site. Only the ids differ (`SLASH` 87/81/90/87, `Regex`
// 224/250/264/225) and the macro names both through the enum, so no
// invocation mentions either. See the arms themselves for the
// fabrication and the missing-operand halves.
macro_rules! impl_js_family_get_op_type {
    (
        $lang:ident,
        op_extras: [$($op_extra:ident),* $(,)?],
        operand_extras: [$($operand_extra:ident),* $(,)?]
        $(, predefined_void: $predefined_type:ident)? $(,)?
    ) => {
        fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
            use $lang::*;

            // TS/TSX only: a `void` return / parameter type is parsed as a
            // `predefined_type` wrapper around an inner `void` token. Both
            // the wrapper (routed through `is_primitive` into the text-keyed
            // `primitive_operators` map as `"void"`) and the inner `Void`
            // token (a standalone expression operator, e.g. `void 0`) would
            // otherwise classify as operators, double-counting one source
            // `void` as two Halstead operators (issue #453). Only `void` has
            // an operator-kind child; the `string` keyword's child is an
            // operand-kind alias, which collided in the other direction
            // (operator + operand) until #1261 dropped it from the operand
            // extras. Suppress the wrapper here and
            // let the inner `Void` token carry the single operator, keeping
            // the kind_id-keyed count consistent with expression `void 0`
            // (the lesson-4 `n1 == dedupe(ops.operators)` invariant).
            $(
                if node.kind_id() == $predefined_type as u16
                    && node
                        .child(0)
                        .is_some_and(|child| child.kind_id() == Void as u16)
                {
                    return TokenRole::Unknown;
                }
            )?

            match node.kind_id().into() {
                // Regex delimiter punctuation. A `regex` literal spells
                // both of its delimiters with the same kind id real
                // division uses, so `const a = /abc/g;` reported a `/`
                // operator with no division in the source and n1/N1
                // counted the literal's punctuation as arithmetic
                // (#1314, the JS-family sibling of Elixir #1256 and
                // Ruby/Perl #1312). The `Regex` node itself is the
                // operand (below), so the delimiters are suppressed
                // exactly when their parent is that node — the
                // compound-leaf guard of grammar-dispatch section 5.
                //
                // Parent, not ancestor, for correctness by
                // construction rather than by observation: unlike
                // Ruby's `#{…}`, a JS regex admits no nested
                // expression at all (`regex_pattern` and `regex_flags`
                // are leaves), so no fixture can tell this guard from
                // an ancestor-scanning one. The halstead test
                // `js_regex_delimiter_guard_is_parent_scoped_is_unobservable`
                // records that, and pins the grammar property it rests
                // on, rather than implying coverage the suite lacks.
                //
                // `SLASH2` — the aliased regex-start token every one of
                // these grammars carries — needs no arm: it is absent
                // from the operator list below, so it already lands on
                // `Unknown`, which is what a delimiter should be. That
                // is the one place this differs from Ruby's guard,
                // where `SLASH2` had to be moved off the arithmetic arm.
                SLASH
                    if ancestors.parent_has_kind(node, Regex as u16) =>
                {
                    TokenRole::Unknown
                }
                Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
                | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If
                | Else | Switch | Case | Default | Async | Do | For | In | Of | While | Try
                | Catch | Finally | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH
                | PLUSPLUS | SLASH | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ
                | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ
                | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ
                | CARET | CARETEQ | PIPEEQ | Yield | LBRACK | LBRACE | Await | QMARK
                | QMARKQMARK | EQGT | DOTDOTDOT | New | Let | Var | Const | Function
                | FunctionExpression | SEMI | Typeof | Instanceof | Void
                // `get`/`set` accessor keywords are operators, matching the
                // C# getter's `Get | Set | Init | Add | Remove` accessor arm.
                // Before #695 the JS family classified them as operands, so
                // the same accessor keyword landed in opposite Halstead
                // groups across languages, skewing n1/n2 for accessor-heavy
                // code (#695).
                | Set | Get
                $(| $op_extra)* => TokenRole::Operator,
                // `Regex` is the literal's own node and contributes one
                // operand, the way Ruby's `Regex` and Elixir's `Sigil`
                // do. It was in neither arm before #1314, so `/abc/g`
                // reached the vocabulary from *neither* side: a
                // fabricated `/` operator and no operand at all. Its
                // `regex_pattern` / `regex_flags` children stay
                // unclassified, so the wrapper cannot double-count
                // them, and no interpolation is possible inside a JS
                // regex — hence a plain arm here rather than the
                // `string_operand_type` dispatch the template literal
                // below needs.
                // `PrivatePropertyIdentifier` is the `#x` leaf of a
                // private class field, in both its declaration
                // (`#x = 1`) and its access (`this.#x`). See the
                // leaves-not-composites note above the macro for why
                // it had to be added in the same change that dropped
                // `MemberExpression*`.
                //
                // `MetaProperty` is `import.meta` / `new.target`: one
                // atomic operand, like `this`, whose `meta` / `target`
                // leaves are anonymous tokens no arm classifies. It is
                // the one composite the #1263 drop has to keep
                // (grammar-dispatch §6) — without it the meta-object
                // contributes no operand at all while `this.env.x`
                // still yields three.
                Identifier | PropertyIdentifier | PrivatePropertyIdentifier | MetaProperty
                | String | Number | True | False | Null | This | Super | Undefined | Regex
                $(| $operand_extra)* => TokenRole::Operand,
                // A `` `...` `` is a string literal; without interpolation it
                // mirrors `"..."` and contributes one operand. When it has a
                // `TemplateSubstitution` child the inner expression is already
                // walked and classified, so counting the wrapper too would
                // double-count its contribution to `N2` (issue #192, same
                // pattern as #183 C# / #191 Kotlin / #199 Perl).
                TemplateString => {
                    Self::string_operand_type(node, &[TemplateSubstitution as u16])
                }
                _ => TokenRole::Unknown,
            }
        }
    };
}

/// The default space name: the node's `name` field, else `<anonymous>`.
///
/// A free function as well as the trait default so a language that needs
/// to name a *few* kinds specially can delegate the rest rather than
/// restate the rule — `<get>` / `<set>` / `<init>` / `<static-init>` in
/// Kotlin, Java and Groovy all do (#1184). Calling `Self::…` there would
/// recurse.
#[must_use]
pub fn default_func_space_name<'a, 'tree>(
    node: &Node<'tree>,
    code: &'a [u8],
    _ancestors: Ancestors<'tree, '_>,
) -> Option<&'a str> {
    // we're in a function or in a class
    if let Some(name) = node.child_by_field_name("name") {
        node_text(code, &name)
    } else {
        Some("<anonymous>")
    }
}

/// Per-language accessors the space walker and the Halstead
/// operator/operand classification dispatch through.
///
/// # Precondition
///
/// Every method taking a `code: &[u8]` next to a `&Node` slices `code`
/// by that node's byte range. `code` must be the exact buffer `node` was
/// parsed from — `big_code_analysis::Ast::source` for a node obtained from the
/// same `big_code_analysis::Ast`. Pairing a node with any other buffer reads the
/// wrong bytes at best and panics on an out-of-bounds index at worst;
/// the same precondition is documented on [`crate::dump_node`] (#795).
/// `node_text` — reached from the default `get_func_space_name` and
/// from the per-language `get_func_name` overrides — is the sole
/// bounds-checked slice; see its docs for why the rest deliberately
/// are not.
#[doc(hidden)]
/// The kinds one Tcl-family dialect spells the braced-word construct
/// with, for [`Getter::is_subsumed_braced_word`] and
/// [`Getter::braced_word_op_type`]. A struct rather than a row of `u16`
/// parameters because the ids are same-typed and positional: a
/// transposed pair compiles and silently inverts the rule for that
/// dialect.
pub struct BracedWordKinds {
    /// `braced_word_simple`, the literal *value* form.
    pub value: u16,
    /// `braced_word`, the *script* form.
    pub script: u16,
    /// `comment`, the one named child of a script that is not a
    /// command.
    pub comment: u16,
    /// `command`, the generic command node. Its `name` field carries
    /// the leading word [`Getter::is_value_braced_word`] recognises the
    /// construct by, and it is the only parent a *generic* argument
    /// list hangs from. The hidden `_command` supertype (`Command2` in
    /// both enums) is deliberately absent: the parser never emits it
    /// (grammar-dispatch §2).
    pub command: u16,
    /// `word_list`, a command's `arguments` field — and, in both
    /// grammars, also the argument list of the modelled `namespace`
    /// construct, which is why the rule reads the grandparent rather
    /// than stopping here.
    pub word_list: u16,
    /// `simple_word`, the only spelling of a command name this rule
    /// resolves. A computed name (`$cmd {…}`, `[pick] {…}`) parses as
    /// `variable_substitution` / `command_substitution` and is not
    /// statically resolvable at all.
    pub simple_word: u16,
    /// `argument`, one entry of a `proc` parameter list. A modelled
    /// slot that holds a *value* rather than a script: a defaulted
    /// parameter (`proc p {a {b {x y}}}`) spells its default as a
    /// `braced_word`, and a default is never evaluated as code.
    pub argument: u16,
    /// `procedure`, the modelled `proc` construct. Its `name` field is a
    /// `braced_word` when the name holds a space (`proc {my proc} …`),
    /// and a name is a literal although the construct's other braced
    /// slot, the body, is a script
    /// ([`Getter::is_braced_literal_slot`]).
    pub procedure: u16,
    /// `namespace`, the modelled construct — not the keyword token that
    /// shares its name. Its `word_list` holds a subcommand followed by
    /// that subcommand's arguments, and only three subcommands take a
    /// script ([`Getter::is_braced_literal_slot`]).
    pub namespace: u16,
    /// `{`, the brace opener — the *only* node
    /// [`Getter::braced_word_op_type`] may revise. A braced word's
    /// operator children are not the opener alone: `_terminator` is a
    /// hidden rule, so a `;` separating two commands is inlined as a
    /// direct child of the `braced_word` and both dialects classify it
    /// as an operator. Keying the revision on the parent kind alone
    /// therefore swallowed those separators too, taking
    /// `lappend x {puts a ; puts b}` to `n1` 0 / `N1` 0 and
    /// `halstead.effort` — a gated threshold metric — to `0.0`.
    pub open_brace: u16,
}

/// The core Tcl-family commands that evaluate a braced argument as a
/// *script* and that neither dialect's grammar models with a node of its
/// own (#1318).
///
/// A command the grammar *does* model — `proc`, `if`, `while`,
/// `foreach`, `catch`, `try`, `namespace`, iRules' `when`, `for`,
/// `switch` and `dict for` / `dict update` / `dict with` — needs no
/// entry: its body is a child of that construct's own node rather than
/// of a generic `command`, which
/// [`Getter::generic_argument_command`] already answers `None` for.
/// `for` and `switch` appear here because the *Tcl* grammar models
/// neither (#467, #1264); the iRules grammar models both, so those two
/// rows are live for one dialect and inert for the other.
///
/// Each entry is a command whose documented syntax puts a script in a
/// braced argument:
///
/// | command | syntax |
/// | --- | --- |
/// | `after` | `after ms script` |
/// | `eval` | `eval arg ?arg …?` |
/// | `for` | `for start test next body` — all four are evaluated |
/// | `on` | `on code varList script`, a `try` handler clause |
/// | `switch` | `switch ?options? string pattern body ?pattern body …?` |
/// | `time` | `time script ?count?` |
/// | `trap` | `trap pattern varList script`, a `try` handler clause |
/// | `uplevel` | `uplevel ?level? arg ?arg …?` |
///
/// `on` and `trap` are listed because the iRules grammar models
/// `on_handler` / `trap_handler` only *under* `try` (pinned by
/// `irules_try_handler_kinds_appear_only_under_try`), and the Tcl
/// grammar models neither a `trap` nor a second `on`; written outside
/// that shape they parse as generic commands. Their last argument is
/// the handler script. The pattern and variable list before it are
/// values, which only [`Getter::is_braced_literal_slot`] tells apart —
/// the `{}` operator this table decides still bills them as blocks.
///
/// `lmap varname list body` is deliberately **not** listed even though
/// its body is a script: the list is per-command, not per-argument, so
/// listing it would bill `lmap i {1 2 3} {…}`'s *list* as a block —
/// the same spelling sensitivity this rule exists to remove, and worse
/// than the one occurrence its body gives up. `for` and `switch` have
/// no such argument (`for`'s four are all evaluated, `switch`'s braced
/// argument is the arm list).
///
/// Subcommand-dispatched script takers (`dict for`, `interp eval`,
/// `trace add … {script}`) are absent for a related reason: the
/// leading word alone cannot tell `dict for` from `dict set`, and
/// admitting it would misclassify the far commoner value-taking
/// spellings. Tk callbacks (`bind`, `fileevent`, a `-command {…}`
/// option) are absent for the same reason as any user proc.
///
/// This list is the whole of the heuristic, and it is knowingly
/// incomplete — see [`Getter::is_value_braced_word`] for what an
/// unlisted command defaults to and why.
const SCRIPT_TAKING_COMMANDS: [&str; 8] = [
    "after",
    "eval",
    "for",
    "on",
    SWITCH_COMMAND,
    "time",
    "trap",
    "uplevel",
];

/// Named because two rules have to agree on it: `switch` takes a
/// script, and its *arm bodies* are scripts too even though the Tcl
/// grammar hangs them off a `command` named after the pattern
/// ([`Getter::is_switch_arm`]). Spelling it twice would let one drift.
const SWITCH_COMMAND: &str = "switch";

/// The `namespace` subcommands with a script argument:
/// `namespace eval ns arg ?arg …?`, `namespace inscope ns script ?arg …?`
/// and `namespace code script`. Every other subcommand takes values —
/// `export {pattern}`, `path {ns …}`, `ensemble create -map {dict}` —
/// which is what [`Getter::is_braced_literal_slot`] keys on.
const NAMESPACE_SCRIPT_SUBCOMMANDS: [&str; 3] = ["eval", "inscope", "code"];

/// The `try` handler clauses a grammar leaves as generic commands:
/// `on code varList script` and `trap pattern varList script`, whose
/// every argument but the last is a value
/// ([`Getter::is_braced_literal_slot`]).
const TRY_HANDLER_COMMANDS: [&str; 2] = ["on", "trap"];

/// Per-language accessors that *name* and *classify* what a node is:
/// the function or space name, the [`SpaceKind`] a node opens, and the
/// [`TokenRole`] of a leaf.
///
/// Every method has a default that answers "nothing" (`None`,
/// [`SpaceKind::Unknown`], [`TokenRole::Unknown`]); a language
/// overrides only the ones its grammar expresses.
pub trait Getter {
    /// Names the function `node` declares. Defaults to
    /// [`get_func_space_name`](Self::get_func_space_name).
    #[must_use]
    fn get_func_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        Self::get_func_space_name(node, code, ancestors)
    }

    /// Names the space `node` opens.
    ///
    /// `ancestors` is the chain the caller descended through. Elixir
    /// needs it: its `def` / `defmodule` heads are ordinary `Call`
    /// nodes, and one inside a `quote` template names no space at all,
    /// which is a question about what encloses the call (#1088).
    #[must_use]
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        default_func_space_name(node, code, ancestors)
    }

    /// The kind of space `node` opens, or [`SpaceKind::Unknown`] when it
    /// opens none.
    #[must_use]
    fn get_space_kind(_node: &Node) -> SpaceKind {
        SpaceKind::Unknown
    }

    /// Source-aware variant of [`get_space_kind`](Self::get_space_kind). The default
    /// forwards to the byte-less classifier; languages whose space
    /// kinds are encoded in macro identifier text (Elixir's
    /// `defmodule` / `def` / `defp` / `defmacro` / `defmacrop` Calls)
    /// override this so the walker can attribute the correct
    /// `SpaceKind` to each promoted func space (#275).
    ///
    /// `ancestors` is the chain the caller descended through; Elixir
    /// needs it to see whether the `Call` sits inside a `quote`
    /// template without paying `Node::parent`'s `O(depth)` (#1084).
    #[inline]
    #[must_use]
    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        Self::get_space_kind(node)
    }

    /// Classifies `node` as a Halstead operator, operand, or neither.
    ///
    /// `ancestors` is the chain the walker descended through. Six
    /// impls read a parent from it to disambiguate a token whose role
    /// depends on what encloses it: Python's `not` / `in` / `is` inside
    /// the compound `not in` / `is not`, Rust's `||` and `!` inside a
    /// binary expression rather than a doc-comment marker, the
    /// namespace identifier in both C++ grammars, Bash's `$name`, and
    /// iRules' `$var`. Reaching those parents with [`Node::parent`]
    /// instead costs `O(depth)` per node (#1096).
    #[must_use]
    fn get_op_type<'a>(_node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> TokenRole {
        TokenRole::Unknown
    }

    /// Source-aware variant of [`get_op_type`]. The default forwards
    /// to the byte-less classifier; languages whose Halstead operand
    /// classification depends on token text override this. Kotlin uses
    /// it to recover the variable in a short-form string template
    /// (`"Hi $name"`), which the grammar emits as bare `string_content`
    /// tokens with no structured interpolation node — the distinction
    /// between an interpolated `$name` and a literal `$5` is only
    /// visible in the source bytes (#454).
    ///
    /// [`get_op_type`]: Self::get_op_type
    #[inline]
    #[must_use]
    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        _code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        Self::get_op_type(node, ancestors)
    }

    /// Returns the source-byte slice used to key a Halstead *operand*.
    /// The default keys on the operand node's full byte range. Kotlin
    /// overrides this to narrow a short-interpolation name token
    /// (`$name`) to its leading identifier prefix, because the grammar
    /// glues trailing inter-segment text onto the name token
    /// (`"$a $b"` → `"a "`); keying the raw bytes would record a
    /// distinct `"a "` operand and break parity with the long `${a}`
    /// form (#454).
    #[inline]
    #[must_use]
    fn get_operand_id<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> &'a [u8] {
        &code[node.start_byte()..node.end_byte()]
    }

    /// Classifies a string-literal `node` as a single Halstead
    /// operand, *unless* it wraps an interpolation child drawn from
    /// `interp_kinds` — in which case the wrapper yields
    /// [`TokenRole::Unknown`] because the inner expressions are
    /// walked and counted separately. Counting the wrapper too would
    /// double-count their contribution to `N2`.
    ///
    /// This declares the per-language interpolation skip once (issue
    /// #420), replacing nine independently-added regression fixes
    /// (#183 / #184 / #191 / #192 / #199 / #277, …). Each language
    /// supplies only its own grammar's interpolation child-kind ids;
    /// the per-call rationale lives at each call site.
    #[must_use]
    fn string_operand_type(node: &Node, interp_kinds: &[u16]) -> TokenRole {
        if node.wraps_any(interp_kinds) {
            TokenRole::Unknown
        } else {
            TokenRole::Operand
        }
    }

    /// Whether `node` is a fragment of a Tcl-family braced word rather
    /// than a token of its own, and so contributes no operand (#1354).
    ///
    /// The two dialects are deliberate clones, so the rule is declared
    /// once and instantiated with each grammar's [`BracedWordKinds`];
    /// the per-call rationale lives at each call site. It answers yes
    /// in two cases:
    ///
    /// - `node` sits directly inside a braced *value*
    ///   (`braced_word_simple`). Tcl evaluates nothing between braces,
    ///   so the value is its whole span and the parts inside it are
    ///   letters — the wrapper carries the one operand. This subsumes
    ///   #1314's narrower guard on the opening `{`, which is one more
    ///   such child.
    /// - `node` is a braced *script* (`braced_word`) holding at least
    ///   one command — a named child other than a comment. Commands are
    ///   what the walk descends into and counts in their own right, so
    ///   billing the block as well would count its whole text a second
    ///   time. A comment is named too, but no arm classifies it, so a
    ///   body holding only comments is billed like an empty one rather
    ///   than like nothing at all.
    ///
    /// A `script` with no command is deliberately not subsumed: the
    /// same kind serves as the value slot of every command the grammar
    /// does not special-case, where `lappend l {}` is an empty list and
    /// the brace pair is its only carrier (grammar-dispatch §6). An
    /// empty or comment-only `proc` body is spelled identically and so
    /// also scores one operand, its whole text. #1318 can now tell the
    /// two apart by the enclosing command, but it bills them alike, so
    /// this arm needs no help from it.
    ///
    /// This is the *byte-less* half of the braced-word rule and it is
    /// not the whole of it. [`braced_word_op_type`] suppresses the `{`
    /// of a script-kind word the enclosing command shows to be a plain
    /// value, and both Tcl-family getters reach it through
    /// [`get_op_type_with_code`], which is what the walk calls
    /// (grammar-dispatch §7). It revises no operand, so this arm's
    /// answers stand unchanged.
    ///
    /// [`braced_word_op_type`]: Self::braced_word_op_type
    /// [`get_op_type_with_code`]: Self::get_op_type_with_code
    #[must_use]
    fn is_subsumed_braced_word<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> bool {
        ancestors.parent_has_kind(node, kinds.value)
            || (node.kind_id() == kinds.script
                && node
                    .children()
                    .any(|child| child.is_named() && child.kind_id() != kinds.comment))
    }

    /// The generic `command` node `word` is an argument of, paired with
    /// that command's own ancestry — or `None` when `word` instead
    /// fills a slot of a construct the grammar models, and so is a
    /// script by construction.
    ///
    /// Both dialects hang a *generic* command's arguments off a
    /// `word_list` and a *modelled* construct's slots off that
    /// construct's own node (`procedure`, `if`, `else`, `elseif`,
    /// `while`, `foreach`, `catch`, `try`, `finally`, `argument`, plus
    /// iRules' `when_event`, `for`, `switch_arm`, `on_handler`,
    /// `trap_handler` and the three `dict` loops). Asking the parent
    /// kind therefore answers "is this a generic argument" without
    /// enumerating the modelled parents — a list that would be a
    /// coverage claim to re-derive on every grammar bump
    /// (grammar-dispatch §1), and that a grammar gaining one more
    /// modelled construct would silently invalidate.
    ///
    /// A `word_list` whose own parent is *not* a `command` is
    /// `namespace`'s argument list — the one modelled construct in
    /// either grammar that reaches its body through a `word_list`. The
    /// grandparent test answers `None` there, so `namespace eval ns
    /// {…}` keeps its body a script.
    ///
    /// A braced word directly under a `command` is the command *name*
    /// (`{puts} hi`), not an argument, and also answers `None`: its
    /// caller decides that case before asking, because a name is
    /// always a literal and must not pick up the `switch`-arm rescue
    /// below.
    #[must_use]
    fn generic_argument_command<'tree, 'chain>(
        word: &Node<'tree>,
        ancestors: Ancestors<'tree, 'chain>,
        kinds: &BracedWordKinds,
    ) -> Option<(Node<'tree>, Ancestors<'tree, 'chain>)> {
        let mut chain = ancestors.iter(word);
        let (parent, _) = chain.next()?;
        if parent.kind_id() != kinds.word_list {
            return None;
        }
        let (grandparent, grandparent_ancestors) = chain.next()?;
        (grandparent.kind_id() == kinds.command).then_some((grandparent, grandparent_ancestors))
    }

    /// Whether a braced *script*-kind word is really a plain value —
    /// the literal `{a b}` of `lappend x {a b}` rather than the block
    /// of `eval {…}` (#1318).
    ///
    /// The two roles share one kind, so no kind-scoped arm can separate
    /// them; recognition is out-of-band, by the enclosing command's
    /// leading word (grammar-dispatch §9). A word that fills a modelled
    /// construct's slot is a script; a word passed to a command in
    /// `SCRIPT_TAKING_COMMANDS` is a script; **everything else is a
    /// value**.
    ///
    /// That default is the load-bearing choice, and it is deliberately
    /// the opposite of "keep today's answer for anything unrecognised":
    ///
    /// - The script-taking set is *closed* and small — Tcl defines no
    ///   user-extensible control structures, so a command that
    ///   evaluates a braced argument is either a core command listed
    ///   above or a proc that forwards to `eval` / `uplevel`. The
    ///   value-taking set is *open*: every user proc and every package
    ///   command that takes a list or a pattern is in it. An
    ///   unrecognised name is therefore far likelier to be value-taking.
    /// - Defaulting to script *fabricates* — it reports a `{}` operator
    ///   for a block the source does not contain. Defaulting to value
    ///   can only *omit*: one `N1` occurrence per script handed to an
    ///   unlisted command, and — when no modelled construct in the same
    ///   space opens a block — the `{}` vocabulary entry with them.
    ///   That last case is where the cost shows: a top-level
    ///   `dict for {k v} $d { puts $k }` with nothing around it has no
    ///   operator left, and Halstead's difficulty is a product with
    ///   `n1` in it, so `effort` reads `0.0` rather than a little low.
    ///   Inside a `proc` the `proc` keyword and body brace keep the
    ///   space non-zero, which is the scope `halstead.effort` is gated
    ///   at; and the same `0.0` was already the answer for a file of
    ///   `set x {a b}` lines, so it is Halstead's shape on an
    ///   operator-free space, not a new failure mode. The omission
    ///   stays bounded only because the answer is scoped to the brace:
    ///   see [`braced_word_op_type`] for the measurement that decided
    ///   it, and for why suppressing the *contents* of a value would
    ///   have made it unbounded instead.
    /// - It stops the score moving with the author's choice of
    ///   delimiter, which is what #695, #1312 and #1314 each restored
    ///   elsewhere: `puts {c d}` and `puts "c d"` now agree on the
    ///   operator column.
    ///
    /// The cost is a script passed to an unlisted command — a Tk
    /// `-command {…}` callback, `trace add variable v w {…}`, a
    /// subcommand dispatch (`dict for`, `interp eval`), a user-defined
    /// `with_lock {…}` — losing the `{}` its block deserves. No
    /// structural signal distinguishes those cases: the grammar parses
    /// `{a b}` and `{puts hi}` into the same shape. The code inside
    /// them is still counted, so the loss is one operator occurrence
    /// per such block, plus the vocabulary entry in the case above.
    ///
    /// [`braced_word_op_type`]: Self::braced_word_op_type
    #[must_use]
    fn is_value_braced_word<'a>(
        word: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> bool {
        // Two positions answer "value" without consulting any command
        // name, and both must be asked before the lookup below,
        // because neither hangs off a `command` the way an argument
        // does — and the second must not reach the `switch`-arm
        // rescue.
        //
        // A defaulted `proc` parameter (`proc p {a {b {x y}}}`) is the
        // one modelled slot holding a value: a default is data the
        // interpreter assigns, never a script it evaluates.
        //
        // A braced word directly under a `command` is that command's
        // *name* (`{puts} hi` invokes `puts`), so it is a literal word.
        // Inside a `switch` arm list the name is the arm's *pattern*,
        // and a braced pattern (`switch -regexp $v { {^a.*b$} {…} }`)
        // is idiomatic — rescuing it as an arm body would fabricate a
        // block around a regex.
        if ancestors
            .parent(word)
            .is_some_and(|parent| [kinds.argument, kinds.command].contains(&parent.kind_id()))
        {
            return true;
        }
        let Some((command, command_ancestors)) =
            Self::generic_argument_command(word, ancestors, kinds)
        else {
            return false;
        };
        if Self::command_leading_word(&command, code, kinds)
            .is_some_and(|name| SCRIPT_TAKING_COMMANDS.contains(&name))
        {
            return false;
        }
        !(Self::is_switch_arm(&command, code, command_ancestors, kinds)
            && Self::is_switch_arm_body(word, &command))
    }

    /// Whether `node` is a Tcl-family braced *script* — a `proc` or `if`
    /// body, an iRules `when` handler — rather than a braced literal
    /// (#1381).
    ///
    /// The positive form of [`is_value_braced_word`], narrowed to the
    /// script kind so it answers `false` for every node of every other
    /// kind and for the two literal spellings, and narrowed again by the
    /// value slots [`is_braced_literal_slot`] recognises. That makes it
    /// the question the *non-Halstead* classifiers ask:
    /// `Checker::is_string_with_code` must not call a `proc` body a
    /// string literal, and `Alterator::keeps_children` must stop the dump
    /// flattening one into a leaf, which dropped the body from it. Both
    /// once listed `braced_word` beside `quoted_word` and
    /// `braced_word_simple`, which is right for the literal role and
    /// wrong for the script role the same kind also serves.
    ///
    /// Stated here rather than in each of the four call sites so the
    /// string and dump classifiers cannot drift apart on the same bytes
    /// (grammar-dispatch §7). Halstead is the deliberate exception:
    /// `braced_word_op_type` asks [`is_value_braced_word`] alone, so the
    /// braces of the value slots `is_braced_literal_slot` adds still
    /// bill a `{}` operator, as they have since #1318. Moving that rule
    /// into the shared predicate changes `bca metrics` for Tcl, and is
    /// its own measured change rather than a rider on this one.
    ///
    /// [`is_value_braced_word`]: Self::is_value_braced_word
    /// [`is_braced_literal_slot`]: Self::is_braced_literal_slot
    #[must_use]
    fn is_braced_script_word<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> bool {
        node.kind_id() == kinds.script
            && !Self::is_value_braced_word(node, code, ancestors, kinds)
            && !Self::is_braced_literal_slot(node, code, ancestors, kinds)
    }

    /// Whether `word`, a braced word [`is_value_braced_word`] calls a
    /// script, fills a slot whose documented syntax takes a *value*. Three
    /// constructs hold both roles in one argument list:
    ///
    /// | construct | value slots | script slot |
    /// | --- | --- | --- |
    /// | `proc name args body` | the `name` field | the body |
    /// | `namespace sub ?arg …?` | any subcommand's but three | `eval`, `inscope`, `code` |
    /// | `on code varList script`, `trap pattern varList script` | all but the last | the last |
    ///
    /// Each literal here was a string and a flat dump leaf before #1381,
    /// and would otherwise have become a script under it: the dump
    /// rendered `{my proc}` as a command named `my`, and
    /// `namespace export {…}` and `namespace ensemble create -map {…}`
    /// both occur in the Tcl 8.6 standard library.
    ///
    /// Two guards keep the construct-wide answer. A switch arm list parses
    /// as commands, so an arm whose *pattern* is spelled `proc`,
    /// `namespace`, `on` or `trap` builds one of these shapes around what
    /// are really arm bodies — [`is_switch_arm`] recognises it first. And
    /// an owner holding a parse error has no argument positions worth
    /// trusting. The multi-line `try … trap` clause is out of reach
    /// entirely: the Tcl grammar leaves it inside an `ERROR` node, where
    /// no role signal survives.
    ///
    /// [`is_value_braced_word`]: Self::is_value_braced_word
    /// [`is_switch_arm`]: Self::is_switch_arm
    #[must_use]
    fn is_braced_literal_slot<'a>(
        word: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> bool {
        let mut chain = ancestors.iter(word);
        let Some((parent, above_parent)) = chain.next() else {
            return false;
        };
        if parent.kind_id() == kinds.procedure {
            let is_name =
                matches!(parent.child_by_field_name("name"), Some(name) if name.id() == word.id());
            return is_name && !Self::is_switch_arm(&parent, code, above_parent, kinds);
        }
        let Some((owner, above_owner)) = chain.next() else {
            return false;
        };
        if parent.kind_id() != kinds.word_list
            || owner.has_error()
            || Self::is_switch_arm(&owner, code, above_owner, kinds)
        {
            return false;
        }
        if owner.kind_id() == kinds.namespace {
            Self::namespace_subcommand_takes_values(&parent, code, kinds)
        } else {
            Self::is_try_handler_value(word, &owner, code, kinds)
        }
    }

    /// Whether a `namespace` construct's `word_list` names a subcommand
    /// whose arguments are values — anything but
    /// `NAMESPACE_SCRIPT_SUBCOMMANDS`. A subcommand that is not a plain
    /// word (`namespace $sub …`) is unresolvable and keeps the script
    /// answer, as an unresolvable command name does.
    #[must_use]
    fn namespace_subcommand_takes_values(
        word_list: &Node<'_>,
        code: &[u8],
        kinds: &BracedWordKinds,
    ) -> bool {
        let Some(subcommand) = word_list.child(0) else {
            return false;
        };
        if subcommand.kind_id() != kinds.simple_word {
            return false;
        }
        let Some(subcommand) = node_text(code, &subcommand) else {
            return false;
        };
        !NAMESPACE_SCRIPT_SUBCOMMANDS.contains(&subcommand)
    }

    /// Whether `word` is an argument of a generic `on` / `trap` command
    /// other than its last, the handler script. The index comes from an
    /// `O(log n)` cursor lookup, as in [`is_switch_arm_body`], not a
    /// sibling scan.
    ///
    /// [`is_switch_arm_body`]: Self::is_switch_arm_body
    #[must_use]
    fn is_try_handler_value(
        word: &Node<'_>,
        command: &Node<'_>,
        code: &[u8],
        kinds: &BracedWordKinds,
    ) -> bool {
        let is_handler = command.kind_id() == kinds.command
            && matches!(
                Self::command_leading_word(command, code, kinds),
                Some(name) if TRY_HANDLER_COMMANDS.contains(&name)
            );
        if !is_handler {
            return false;
        }
        let Some(arguments) = command.child_by_field_name("arguments") else {
            return false;
        };
        let mut cursor = arguments.cursor();
        let index = cursor.goto_first_child_for_byte(word.start_byte());
        cursor.node().id() == word.id()
            && matches!(index, Some(index) if index + 1 < arguments.child_count())
    }

    /// Whether `word`, an argument of a `switch` arm command
    /// ([`is_switch_arm`]), sits in a *body* position rather than a
    /// *pattern* position.
    ///
    /// The arm list is a flat run of `pattern body pattern body …`,
    /// and the grammar breaks it into commands at newlines. One arm per
    /// line gives each command a pattern for its name and a body for
    /// its sole argument; several arms on one line give the first
    /// pattern's command the whole run as arguments, so the arguments
    /// alternate `body pattern body …`. The command-name test above
    /// keeps the *first* braced pattern a literal, but without this
    /// gate every later one was rescued as a body, and a one-line
    /// `switch -regexp $v { {^a} {…} {^b} {…} }` fabricated a `{}`
    /// around `{^b}` that the same arms written one per line did not —
    /// the score moving with layout, which is what the rule exists to
    /// stop.
    ///
    /// Position is the only signal because it is what Tcl itself uses:
    /// `switch` pairs the list up by index, with no marker on either
    /// half. The parity holds across the words that can interpose — a
    /// `-` fall-through body and a `default` pattern are both
    /// `simple_word`s and take a slot each — so an even index is a body
    /// and an odd one a pattern. This is the one place the rule needs a
    /// sibling *index* rather than a parent kind, and it runs once per
    /// braced argument of such a command.
    ///
    /// The index comes from [`Cursor::goto_first_child_for_byte`], which
    /// is `O(log n)` in the argument count. A `children().position(..)`
    /// scan answers identically in `O(n)`, and since every braced word
    /// of a one-line arm list asks, that made the Halstead walk — and,
    /// once #1381 routed them through this rule, `find` / `count
    /// --type string` and the `Ast` dump — quadratic in the width of
    /// one line: seconds per request at tens of KB (#1381 review). The
    /// id check keeps the scan's answer for a word that is not one of
    /// the command's arguments at all.
    ///
    /// [`is_switch_arm`]: Self::is_switch_arm
    /// [`Cursor::goto_first_child_for_byte`]: crate::node::Cursor::goto_first_child_for_byte
    #[must_use]
    fn is_switch_arm_body(word: &Node<'_>, command: &Node<'_>) -> bool {
        let Some(arguments) = command.child_by_field_name("arguments") else {
            return false;
        };
        let mut cursor = arguments.cursor();
        let index = cursor.goto_first_child_for_byte(word.start_byte());
        cursor.node().id() == word.id() && matches!(index, Some(index) if index.is_multiple_of(2))
    }

    /// A command's leading word, when it is a statically resolvable
    /// `simple_word`.
    ///
    /// Located by field rather than by index (grammar-dispatch §3): the
    /// name is `command`'s `name` field in both grammars. A computed
    /// name (`$cmd {…}`, `[pick] {…}`) parses as a substitution node and
    /// is not resolvable at all, and a name spelled in quotes or braces
    /// (`"eval" {…}`) is legal Tcl that this deliberately leaves
    /// unresolved — the same limitation `tcl_command_name` records for
    /// the Cognitive and Cyclomatic walkers.
    #[must_use]
    fn command_leading_word<'c>(
        command: &Node<'_>,
        code: &'c [u8],
        kinds: &BracedWordKinds,
    ) -> Option<&'c str> {
        let name = command.child_by_field_name("name")?;
        if name.kind_id() != kinds.simple_word {
            return None;
        }
        let text = node_text(code, &name)?;
        // `::eval` *is* `eval` — a leading `::` names the global
        // namespace, and inside a `namespace eval` body it is the
        // spelling that guarantees the core command rather than a local
        // proc shadowing it. Without this the qualified form fell to
        // the value default and lost its block, so the score moved with
        // how the author spelled a command that resolves identically.
        //
        // Only the *leading* qualifier is stripped: `ns::eval` is a
        // different command living in `ns`, and must not be mistaken
        // for the core one.
        Some(text.strip_prefix("::").unwrap_or(text))
    }

    /// Whether `command` is really one `pattern body` pair of a Tcl
    /// `switch` arm list rather than a command of its own.
    ///
    /// Tcl models no `switch`, so `switch $v {a {…} b {…}}` parses the
    /// arm list as a braced word whose interior is a `command` named
    /// after the *first pattern* — `a` here — with the arm bodies as
    /// its arguments (grammar-dispatch §9; the Cognitive walker reads
    /// the same shape through `tcl_switch_arm_list`). Without this
    /// test the bodies would read as literals passed to a command
    /// called `a`, which is exactly what the value default is for
    /// everywhere else. iRules models `switch` with `switch_arm`
    /// children, so its arm bodies never reach here.
    #[must_use]
    fn is_switch_arm<'a>(
        command: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> bool {
        ancestors
            .iter(command)
            .next()
            .is_some_and(|(arm_list, above)| {
                arm_list.kind_id() == kinds.script
                    && Self::generic_argument_command(&arm_list, above, kinds)
                        .and_then(|(switch, _)| Self::command_leading_word(&switch, code, kinds))
                        == Some(SWITCH_COMMAND)
            })
    }

    /// Whether the `{` of `node`'s parent braced word opens a block,
    /// or merely quotes a literal (#1318).
    ///
    /// This is #1314's guard with its kind test replaced by a role
    /// test. #1314 suppressed the opener of a `braced_word_simple`,
    /// the literal form the grammars emit only in the value slots they
    /// special-case; everywhere else a literal is a `braced_word`, the
    /// same kind a block uses, and its `{` still reported a `{}`
    /// operator for a block the source does not contain —
    /// `lappend x {a b}`, `puts {c d}`, and every user proc taking a
    /// list.
    ///
    /// **It revises the operator only, and deliberately leaves the
    /// words inside a value alone.** Suppressing them too would make
    /// `lappend x {a b}` score like its `set x {a b}` synonym, which
    /// is the tidier answer for that line — and the wrong one in
    /// general, because the same braced argument of an unrecognised
    /// command is just as often real code: an `oo::class create C {…}`
    /// body, a `tcltest` `-body {…}`, an `apply {{x} {…}}` lambda.
    /// Suppressing contents was built and measured on a file holding
    /// one of each; it took n1 5 → 2, N1 8 → 2, n2 25 → 15 and
    /// N2 32 → 15, the class body, the test body and the lambda each
    /// collapsing into a single operand, and `halstead.effort` — a
    /// gated threshold metric — collapsing with them. So the rule only
    /// ever withdraws a claim the classifier cannot support; it never
    /// discards code the walk has already read. The residual is the
    /// asymmetry the issue opens with: a braced value scores one
    /// operand where the grammar names it (`set`) and one per word
    /// where only the command name would (`lappend`). Closing that
    /// needs a signal neither grammar gives — filed as #1382.
    ///
    /// Keeping to the operator also keeps the whole thing cheap: only
    /// a braced word's own opener can change answer, so the test is one
    /// kind comparison and one parent lookup, the same scope #1354 and
    /// #1314 use — with the single exception of a `switch` arm
    /// command, where [`is_switch_arm_body`] needs the word's index
    /// among that command's arguments, an `O(log n)` cursor lookup. An
    /// ancestor scan would have been `O(depth)` per node and quadratic
    /// on a deeply nested `expr`, the shape #1122 warns about.
    ///
    /// [`is_switch_arm_body`]: Self::is_switch_arm_body
    ///
    /// [`get_op_type`]: Self::get_op_type
    #[must_use]
    fn braced_word_op_type<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kinds: &BracedWordKinds,
    ) -> TokenRole {
        let base = Self::get_op_type(node, ancestors);
        // Only the opener can change answer, and the test has to say so
        // rather than infer it from the parent kind. A braced word's
        // operator children are the `{` *and* every `;` separating two
        // of its commands — `_terminator` is a hidden rule, so the
        // separator is inlined as a direct child of the `braced_word`
        // and both dialects list `SEMI` as an operator. The closer is
        // unclassified and a `\n` terminator is not an operator, so
        // those two are the whole set.
        if node.kind_id() != kinds.open_brace || !matches!(base, TokenRole::Operator) {
            return base;
        }
        let quotes_a_literal = ancestors.iter(node).next().is_some_and(|(parent, above)| {
            parent.kind_id() == kinds.script
                && Self::is_value_braced_word(&parent, code, above, kinds)
        });
        if quotes_a_literal {
            TokenRole::Unknown
        } else {
            base
        }
    }

    /// The grammar's name for the operator kind `id`, used to render a
    /// Halstead operator; empty for languages that do not report them.
    #[must_use]
    fn get_operator_id_as_str(_id: u16) -> &'static str {
        ""
    }
}

mod bash;
mod c;
mod ccomment;
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod irules;
mod java;
mod javascript;
mod kotlin;
mod lua;
mod mozcpp;
mod mozjs;
mod objc;
mod perl;
mod php;
mod preproc;
mod python;
mod ruby;
mod rust;
mod tcl;
mod tsx;
mod typescript;

#[cfg(test)]
mod node_text_tests {
    use super::node_text;
    use crate::langs::RustParser;
    use crate::traits::ParserTrait;
    use std::path::PathBuf;

    /// A node whose span lies inside the buffer it was parsed from
    /// yields its exact source text.
    #[test]
    fn in_bounds_span_returns_text() {
        let src = "fn x() {}";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert_eq!(node_text(parser.code(), &root), Some(src));
    }

    /// Reslicing a node against a *shorter* buffer (the stale-span hazard
    /// the guard exists for) must degrade to `None`, not panic. A direct
    /// `&code[start..end]` would panic here — this is the revert check.
    #[test]
    fn out_of_bounds_span_returns_none_not_panic() {
        let src = "fn x() {}";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert!(root.end_byte() > 2);
        let truncated = &src.as_bytes()[..2];
        assert_eq!(node_text(truncated, &root), None);
    }

    /// The UTF-8 guard is the *other* failure mode, and unlike the range
    /// guard it is reachable without violating the same-parse
    /// precondition: `Ast::parse` accepts arbitrary bytes. A span whose
    /// bytes are not valid UTF-8 must yield `None`, not a panic and not
    /// lossy replacement characters.
    #[test]
    fn non_utf8_span_returns_none() {
        let mut src = b"fn ".to_vec();
        src.extend_from_slice(&[0xF0, 0x9F]);
        src.extend_from_slice(b"() {}");
        let parser = RustParser::new(src.clone(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert_eq!(root.end_byte(), src.len());
        assert_eq!(node_text(parser.code(), &root), None);
    }
}

#[cfg(test)]
mod ancestor_tests {
    use super::Getter;
    use crate::node::{Ancestors, Node};
    use crate::test_support::for_each_node_with_chain;
    use crate::traits::LanguageInfo;

    /// `get_func_space_name` must name a space the same whether it reads
    /// the walker's ancestor chain or climbs with `Node::parent`.
    ///
    /// Two grammars consult an ancestor here, for different reasons:
    /// the JS family names an anonymous `function` / arrow from the
    /// `pair` or `variable_declarator` holding it, and Elixir skips
    /// naming a `def` that sits inside a `quote` template. #1088 moved
    /// both onto the chain.
    fn assert_name_parity<L: LanguageInfo + Getter>(
        label: &str,
        code: &[u8],
        expect_named: &[&str],
    ) {
        let mut seen: Vec<&str> = Vec::new();
        let visited = for_each_node_with_chain::<L>(code, |node: &Node<'_>, chain| {
            let known = L::get_func_space_name(node, code, Ancestors::known(chain));
            let climbing = L::get_func_space_name(node, code, Ancestors::unknown());
            assert_eq!(
                known,
                climbing,
                "{label}: name of {} at row {} disagrees",
                node.kind(),
                node.start_row()
            );
            if let Some(name) = known
                && expect_named.contains(&name)
                && !seen.contains(&name)
            {
                seen.push(name);
            }
        });
        assert!(visited > 20, "{label}: fixture is too small to prove much");
        for name in expect_named {
            assert!(
                seen.contains(name),
                "{label}: no node resolved to {name:?}, so the fixture no longer \
                 exercises the ancestor-derived naming it was added for"
            );
        }
    }

    #[test]
    fn func_space_name_agrees_between_known_and_climbing() {
        // `outer` and `keyed` are only reachable through the parent:
        // the function expressions themselves carry no `name` field.
        //
        // All four JS-family grammars are exercised, not just
        // JavaScript: their `get_func_space_name` impls are separate
        // copies of the same body against four distinct `kind_id`
        // enums, so a `Pair` / `VariableDeclarator` id that drifted in
        // one grammar would be invisible here if only one were checked.
        let js_source =
            b"var outer = function () { return 1; };\nvar o = { keyed: function () { return 2; } };\n";
        assert_name_parity::<crate::langs::JavascriptCode>(
            "javascript",
            js_source,
            &["outer", "keyed"],
        );
        assert_name_parity::<crate::langs::MozjsCode>("mozjs", js_source, &["outer", "keyed"]);
        assert_name_parity::<crate::langs::TypescriptCode>(
            "typescript",
            js_source,
            &["outer", "keyed"],
        );
        assert_name_parity::<crate::langs::TsxCode>("tsx", js_source, &["outer", "keyed"]);
        // `multi` is named from its `Call` head; the `def a` inside the
        // `quote` template is not a definition, so it falls through to
        // the field-less default.
        assert_name_parity::<crate::langs::ElixirCode>(
            "elixir",
            b"defmodule Foo do\n  defmacro multi do\n    quote do\n      def a, do: 1\n    end\n  end\nend\n",
            &["Foo", "multi"],
        );
    }
}
