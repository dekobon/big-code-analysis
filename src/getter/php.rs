//! `Getter` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for PhpCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            // PHP traits are class-like mixins whose method
            // implementations roll up into the consuming class's WMC; we
            // map them to `SpaceKind::Class` so the per-class metrics
            // (NPA, NPM, WMC) treat them uniformly. The output may label
            // them "class" — that is intentional for metric coherence.
            // LOAD-BEARING: `Wmc::compute` for PhpCode does not match
            // `SpaceKind::Trait`. If you remap `TraitDeclaration` here,
            // also update `src/metrics/wmc.rs`.
            Php::ClassDeclaration
            | Php::AnonymousClass
            | Php::EnumDeclaration
            | Php::TraitDeclaration => SpaceKind::Class,
            Php::InterfaceDeclaration => SpaceKind::Interface,
            Php::FunctionDefinition
            | Php::MethodDeclaration
            | Php::AnonymousFunction
            | Php::ArrowFunction => SpaceKind::Function,
            Php::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Php::*;
        match node.kind_id().into() {
            // String-interpolation opener. `LBRACE` is *both* the
            // compound-statement brace and the complex-interpolation
            // opener, so `"dq {$y} end"` reported a `{}` operator —
            // and reported it against the same vocabulary entry a real
            // block uses, which no other language does (#1314).
            //
            // An interpolation opener is spelling, not an operation:
            // the interpolated expression's own operators are already
            // counted, and no reader performs a `{`. Measured across
            // the six interpolating languages here, `[n1, N1]` on one
            // fixture with and without an interpolation: kotlin
            // [5,7]->[5,7], csharp [8,11]->[8,11], groovy's `${` (142)
            // absent from its operator arm, ruby [1,2]->[2,3], php
            // [2,4]->[3,5], and elixir counting it through the same
            // `HASHLBRACE` token as Ruby. Three of six already
            // declined to count it; this arm plus the Ruby and Elixir
            // `#{` drops on this branch make it six of six.
            //
            // Four parents, not one. The opener is a direct child of
            // `encapsed_string` (`"{$y}"`), of `heredoc_body` — the
            // body, not `heredoc` — of `shell_command_expression`
            // (`` `ls {$y}` ``), and of `dynamic_variable_name`, which
            // covers both the in-string `"${y}"` form and a bare
            // `${$y}` variable-variable. `"${y}"` is deprecated as of
            // PHP 8.2 and removed in 9.0, but the grammar still parses
            // it and it is still in the wild.
            //
            // Parent, not ancestor, and PHP is one of only two
            // languages in #1314 where that is observable: a closure
            // inside an interpolation (`"{$o->m(function() { … })}"`)
            // puts a compound-statement brace under an
            // `encapsed_string` ancestor. Pinned by
            // `php_interpolation_guard_is_parent_scoped_not_ancestor_scoped`.
            //
            // A literal brace in string text (`"lit { brace"`) never
            // reaches here — the grammar folds it into `string_content`
            // — so nothing is over-suppressed.
            LBRACE
                if matches!(
                    ancestors.parent(node).map(|p| p.kind_id().into()),
                    Some(
                        EncapsedString
                            | HeredocBody
                            | ShellCommandExpression
                            | DynamicVariableName
                    )
                ) =>
            {
                HalsteadType::Unknown
            }
            // Operator: control-flow keywords
            If | Else | Elseif | Endif
            | Switch | Case | Default | Endswitch
            | For | Endfor | Foreach | Endforeach
            | While | Endwhile | Do
            | Break | Continue
            | Return | Throw | Try | Catch | Finally
            | Match | Yield | Yieldfrom | Goto
            | Echo | Exit | Print
            | Include | IncludeOnce | Require | RequireOnce

            // Operator: declaration keywords
            | Function | Class | Interface | Trait | Enum | Namespace
            | Use | Const | Global | Static | VarModifier
            | Public | Protected | Private
            | Final | Abstract | Readonly
            | New | Clone | Instanceof | As | Insteadof | Extends | Implements
            | Fn | Declare | Enddeclare | Unset | List
            | Zelf | Parent

            // Operator: structural punctuation. Only the *opening*
            // delimiter is an operator (the pair folds to one glyph in
            // `get_operator_id_as_str`); the former closing arms
            // (`RBRACE`/`RPAREN`/`RPAREN2`/`RBRACK`) double-counted every
            // balanced pair, inflating n1/N1 (#695). `LPAREN2` is defensive —
            // the runtime collapses it to `LPAREN` before `kind_id()`, so it
            // never fires (#768; see the Cpp note).
            | LBRACE | LPAREN | LPAREN2
            | LBRACK
            | COMMA | SEMI | COLON | COLONCOLON
            | DASHGT | QMARKDASHGT | EQGT | BSLASH | DOTDOTDOT | QMARK | AT
            | HASHLBRACK

            // Operator: arithmetic
            | PLUS | DASH | STAR | SLASH | PERCENT | STARSTAR
            | PLUSPLUS | DASHDASH

            // Operator: comparison
            | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTGT
            | LT | GT | LTEQ | GTEQ | LTEQGT

            // Operator: logical
            | AMPAMP | PIPEPIPE | BANG
            | And | Or | Xor | QMARKQMARK

            // Operator: bitwise
            | AMP | PIPE | CARET | TILDE | LTLT | GTGT

            // Operator: assignment
            | EQ
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ
            | DOTEQ | QMARKQMARKEQ
            | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ

            // Operator: string concat
            | DOT
                => HalsteadType::Operator,

            // `name` is a genuine operand almost everywhere — function,
            // class, const, member (`->prop`) and namespace-component
            // identifiers are all this kind. But it is ALSO the identifier
            // leaf of every variable reference: `$x` parses as a
            // `variable_name` wrapping `name`, and `"${x}"` as a
            // `dynamic_variable_name` wrapping it directly. Both wrappers
            // are operands in their own right below (keyed with the `$`
            // sigil, the way Bash and iRules key theirs), so counting the
            // leaf too double-counts every variable reference — inflating
            // N2 and adding a spurious sigil-less twin to n2 (#1259).
            // Exclude it exactly in those two positions, the same
            // parent-scoped guard Bash applies to `variable_name` under
            // `simple_expansion` and iRules to `Id` under
            // `variable_substitution`. `Name2` is the hidden `_name`
            // supertype the parser never emits — pinned by
            // `php_name2_hidden_rule_drift_marker` in `src/metrics/abc.rs`.
            // It rides along defensively so a grammar bump that promotes
            // it inherits the guard rather than the bug.
            Name | Name2 => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(VariableName | DynamicVariableName) => HalsteadType::Unknown,
                _ => HalsteadType::Operand,
            },

            // Variable-variable syntax nests these wrappers: `$$x` is a
            // `dynamic_variable_name` around a `variable_name`, `${$x}`
            // the same, and `$$$x` a `dynamic_variable_name` around
            // another `dynamic_variable_name`. Each level's text span
            // contains the one below it, so only the outermost may count;
            // suppressing both kinds under a `dynamic_variable_name`
            // parent handles arbitrary depth (#1259). An inner reference
            // reached through a real expression (`${$x . 'y'}`, whose
            // `$x` hangs off a `binary_expression`) is unaffected.
            VariableName | DynamicVariableName => {
                match ancestors.parent(node).map(|p| p.kind_id().into()) {
                    Some(DynamicVariableName) => HalsteadType::Unknown,
                    _ => HalsteadType::Operand,
                }
            }

            // `named_type`, `optional_type`, `union_type`,
            // `intersection_type`, `disjunctive_normal_form_type`,
            // `qualified_name`, `relative_name` and `namespace_name`
            // are deliberately absent from the operand arm below and
            // fall through to the `_ => Unknown` catch-all (#1293).
            // They cannot be spelled as an explicit `Unknown` arm —
            // `clippy::match_same_arms` rejects one that duplicates the
            // catch-all — so the rationale lives here.
            //
            // Each of them spans exactly the text of the node(s) it
            // contains plus separator glyphs that `get_op_type` already
            // counts as *operators* — `?` (`QMARK`), `|` (`PIPE`), `&`
            // (`AMP`), the DNF parens, and `\` (`BSLASH`). Counting the
            // wrapper as well billed the same bytes two to five times:
            // `?A\B` nests `optional_type` → `named_type` →
            // `qualified_name` → `namespace_name` → `name` and scored 6
            // operands for two identifiers.
            //
            // Both design calls this needed, recorded so the next
            // reader does not have to re-derive them:
            //
            // * A type's operand is the innermost concrete type, not
            //   the decorated form: `?int` is the operand `int`, not
            //   `?int`. The `?` is already an operator, so folding it
            //   into the operand text bills nullability across both
            //   Halstead halves and splits `int` and `?int` into two
            //   vocabulary entries for one type.
            // * A qualified name's operands are its components, not
            //   the whole path: `Foo\Bar\Baz` is `Foo`, `Bar`, `Baz`
            //   with two `\` operators. That is exactly how PHP's own
            //   sibling separators already read here — `Foo::bar`
            //   yields operands `Foo` / `bar` around a `::` operator,
            //   `$o->prop` yields `$o` / `prop` around `->` — and how
            //   Rust's `foo::bar::baz` reads. The whole-path reading
            //   would instead plant a distinct vocabulary entry per
            //   prefix (`Foo`, `Foo\Bar`, `Foo\Bar\Baz`).
            //
            // Dropping these kinds rather than gating a leaf is safe
            // here, unlike `primitive_type` below: every one of them is
            // grammatically required to contain the node that now
            // carries the operand (a `named_type` always wraps a `name`
            // or `qualified_name`, a `namespace_name` always holds at
            // least one `name`), so no spelling regresses to zero the
            // way grammar-dispatch §6 warns about.

            // The primitive-type keyword tokens, suppressed under a
            // `primitive_type` wrapper (#1293). Here the wrapper is the
            // node that carries the operand and the leaf is the
            // duplicate — the opposite of the qualified-name arm above,
            // and deliberately so: `primitive_type` is *childless* for
            // `callable`, `iterable`, `mixed`, `void`, `false` and
            // `true` (verified with `bca dump`; the grammar emits no
            // token node for those spellings), so dropping the wrapper
            // would score those six types zero — grammar-dispatch §6
            // exactly. The wrapper's text equals the keyword's, so
            // `n2` is unchanged and only the duplicate `N2` occurrence
            // goes away.
            //
            // Parent-scoped, not a blanket exclusion: `array` is also
            // the head token of `array(1, 2)` and must keep counting
            // there, and a `(string)` cast is a childless `cast_type`
            // that never reaches this arm.
            //
            // Cross-walk (grammar-dispatch §7): `Checker::is_string`
            // and the PHP `Alterator` arm both list `String2`, so a
            // `: string` return type still answers `is_string`. That
            // stays in step with Halstead because the *span* keeps its
            // operand — the enclosing `primitive_type` carries it — and
            // only the node it is attributed to moved.
            Int | Bool | Array | Object | String2 | Float2 | Null2 => {
                match ancestors.parent(node).map(|p| p.kind_id().into()) {
                    Some(PrimitiveType) => HalsteadType::Unknown,
                    _ => HalsteadType::Operand,
                }
            }

            // Operands: literals and the type nodes that are leaves or
            // carry the operand themselves. `String` (368) is the
            // single-quoted string literal and `String3` the hidden
            // `_string` supertype — the `string` *type* keyword is
            // `String2` (25), handled above. Neither `String` nor
            // `Nowdoc` ever interpolates, so both are always one
            // operand. `bottom_type` (`never`) and `cast_type`
            // (`(int)`) are leaves with no inner node to double-count.
            Integer | Float
            | String | String3
            | Nowdoc
            | Boolean | Null
            | BottomType
            | PrimitiveType | CastType
                => HalsteadType::Operand,

            // `EncapsedString` (double-quoted), `Heredoc`, and
            // `ShellCommandExpression` (backticks) count as one
            // operand when inert. When they carry a `$var`,
            // `${name}`, or `{$expr}` interpolation child, those
            // inner expressions are already walked and classified as
            // operands in their own right; counting the wrapping
            // literal too would double-count their contribution to
            // `N2` (issue #184, same pattern as #180 for Elixir/Bash
            // and #183 for C#). `ShellCommandExpression` was previously
            // omitted entirely (issue #288), so backtick literals
            // contributed no Halstead operand at all even when inert.
            EncapsedString | Heredoc | ShellCommandExpression => {
                // PHP's interpolation children appear directly on the
                // wrapping literal, except `Heredoc`, which holds them
                // one level down under a single `heredoc_body` child —
                // so the descend below mirrors the original
                // `php_string_has_interpolation` two-level walk.
                const PHP_INTERP_KINDS: &[u16] = &[
                    // `"$name"` → direct `variable_name` child.
                    VariableName as u16,
                    // `"${name}"` → direct `dynamic_variable_name` child.
                    DynamicVariableName as u16,
                    // `"$arr[0]"` → direct `subscript_expression` child.
                    // The grammar gives this kind three numeric aliases.
                    SubscriptExpression as u16,
                    SubscriptExpression2 as u16,
                    SubscriptExpression3 as u16,
                    // `"$obj->prop"` → direct `member_access_expression`
                    // child. PHP's bare-interpolation syntax does not
                    // support `?->` (nullsafe) or `::` (scope), so only
                    // member-access aliases need handling here; nullsafe /
                    // scope forms always go through the `{ … }` wrapper.
                    MemberAccessExpression as u16,
                    MemberAccessExpression2 as u16,
                    MemberAccessExpression3 as u16,
                    // `"{$expr}"` → anonymous `{` (LBRACE) opens the
                    // complex-interpolation wrapper whose body is an
                    // arbitrary expression; the brace appears as a direct
                    // child.
                    LBRACE as u16,
                ];
                // Single pass over the direct children: an interpolation
                // child on the literal itself (EncapsedString /
                // ShellCommandExpression) OR, for Heredoc, one nested under
                // its `heredoc_body` child. Folding both checks into one walk
                // avoids re-scanning the children a second time through
                // `string_operand_type` for an inert heredoc.
                let has_interp = node.children().any(|c| {
                    let kind = c.kind_id();
                    PHP_INTERP_KINDS.contains(&kind)
                        || (kind == HeredocBody as u16 && c.wraps_any(PHP_INTERP_KINDS))
                });
                if has_interp {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Php);
}
