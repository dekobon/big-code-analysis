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
        // FIXME(#1314): `LBRACE` is both the compound-statement
        // brace and the complex-interpolation opener (documented
        // below for the operand decision), so `"dq {$y} end"`
        // fabricates a block. Unlike #1312's Ruby/Perl mirrors this
        // needs a policy call first: Kotlin's `${` and C#'s
        // `interpolation_brace` are distinct kinds and are not
        // counted, while Ruby's `#{` is. Settle the rule across all
        // four before guarding here.
        match node.kind_id().into() {
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

            // Operands: literals, type expressions, and the compound
            // name forms. FIXME(#1293): the type and name wrappers here
            // still double-count the leaves they contain — `?int` scores
            // 3 (`optional_type` + `primitive_type` + the `int` token)
            // and `Foo\Bar\Baz` scores 5. Same class as the guards
            // above, but which node should carry the operand is a
            // per-construct call, so it is tracked apart from #1259.
            // `String`/`String2`/`String3` (single-quoted) and
            // `Nowdoc` never interpolate and are always counted as
            // one operand each.
            Integer | Float | Float2
            | String | String2 | String3
            | Nowdoc
            | Boolean | Null | Null2
            | NamedType | OptionalType | UnionType | IntersectionType
            | DisjunctiveNormalFormType | BottomType
            | PrimitiveType | CastType
            | QualifiedName | RelativeName | NamespaceName
            | Int | Bool | Array | Object
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
