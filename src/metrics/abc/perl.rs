#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{
    Abc, Stats, count_boolean_slot, count_negated_operand, is_operand, last_operand,
    wrapped_operand,
};
use crate::*;

// Fitzpatrick's ABC rules adapted for Perl.
//
// - Assignments: every assignment operator token — plain `=` plus the
//   compound forms `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`, `x=`,
//   `&=`, `|=`, `^=`, `<<=`, `>>=`, `&&=`, `||=`, `//=`, and the
//   bitstring forms `&.=`, `|.=`, `^.=`. Each token fires exactly
//   once per textual occurrence inside a `binary_expression`.
// - Branches: every call expression dispatch — `call_expression_with_*`
//   (bareword / spaced args / args-with-brackets / sub / variable /
//   recursive) plus `method_invocation`. The grammar nests an inner
//   `call_expression_with_bareword` (just the function name)
//   underneath the wrapper kinds carrying argument lists, so we only
//   count `CallExpressionWithBareword` when it stands on its own;
//   when its parent is another call form, the outer wrapper has
//   already contributed the branch.
// - Conditions: numeric and string comparison operators (`==`, `!=`,
//   `<`, `>`, `<=`, `>=`, `<=>`, `eq`, `ne`, `lt`, `gt`, `le`, `ge`,
//   `cmp`, `=~`, `!~`), a bare `/re/` / `m{re}` match against `$_`,
//   the ternary operator (`TernaryExpression`),
//   and each `elsif` / `else` clause of an `if` / `unless`
//   statement. Each condition slot pays one condition unless an arm
//   inside it already did (see `perl_count_condition`).
//
//   The short-circuit and low-precedence logical operators (`&&`,
//   `||`, `//`, `and`, `or`, `xor`) are deliberately NOT counted.
//   See the module-level `Stats` doc-comment for the cross-
//   language policy (Fitzpatrick rules mapped from Figure 2 for C,
//   the closest analogue since the paper does not define rules for
//   Perl; issue #395, walker tracked in #403).
// One step of the value peel (see `PeelStep`).
//
// `Array` is tree-sitter-perl's name for the `(...)` shape used BOTH as
// the `if` / `while` / `unless` / `until` condition wrapper AND as list
// literals `(1, 2, 3)`. In the scalar context a condition imposes, a list
// evaluates to its LAST element, so the peel reads the last operand for
// both shapes: `($a)` → `$a`, `($x, $y)` → `$y`. A statement modifier's
// bare `arguments` list (`g() if $a, $b;`) is the same list without the
// parentheses, and a plain `=` evaluates to its right-hand side, which
// is its last operand too — so `if (my $y = $x > 1)` tests the
// comparison its own arm already counts. `parenthesized_argument` holds
// one operand.
//
// Both spellings of the same negation prove the operand boolean — see
// `ruby_wrapper_operand`; Perl has the identical gap (#1182). Read
// through the grammar's `operator` field, whose type list is
// `! + ++ - -- and not ~`: `-$x` yields a value and stops the peel, and
// tree-sitter-perl spells a low-precedence `$a and $b` as a two-operand
// `unary_expression`, which is a chain rather than a wrapper. Do NOT
// match the hidden `_unary_not` supertype (`P::UnaryNot`), which the
// parser never emits (grammar-dispatch item 2).
//
// No operand is read by index: a comment may sit before or after it
// (`(# c` / `! # c`, #1455).
fn perl_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Perl as P;

    match node.kind_id().into() {
        P::Array | P::Arguments => last_operand(node).map(|o| (o, false)),
        P::ParenthesizedArgument => wrapped_operand(node).map(|o| (o, false)),
        P::BinaryExpression if node.is_child(P::EQ as u16) => {
            last_operand(node).map(|o| (o, false))
        }
        P::UnaryExpression
            if node
                .child_by_field_name("operator")
                .is_some_and(|op| matches!(op.kind_id().into(), P::BANG | P::Not)) =>
        {
            wrapped_operand(node).map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison or match operator (the token arms), a bare
// `/re/` match (its own arm — a peeled slot occupant is never a bound
// pattern or a `split` delimiter, the two shapes that arm skips), a
// ternary (its own arm) or a logical chain, whose operands each pay
// through `perl_count_condition`. The operator is found among the node's
// tokens rather than through its `operator` field, which tree-sitter-perl
// leaves off every comparison and off the `and` it parses as a
// `unary_expression`.
fn perl_condition_scores_itself(expr: &Node) -> bool {
    use Perl as P;

    match expr.kind_id().into() {
        P::TernaryExpression | P::PatternMatcher | P::PatternMatcherM => true,
        P::BinaryExpression | P::UnaryExpression => expr.children().any(|token| {
            matches!(
                token.kind_id().into(),
                P::EQEQ
                    | P::BANGEQ
                    | P::LT
                    | P::GT
                    | P::LTEQ
                    | P::GTEQ
                    | P::LTEQGT
                    | P::Eq
                    | P::Ne
                    | P::Lt
                    | P::Gt
                    | P::Le
                    | P::Ge
                    | P::Cmp
                    | P::EQTILDE
                    | P::BANGTILDE
                    | P::AMPAMP
                    | P::PIPEPIPE
                    | P::SLASHSLASH
                    | P::And
                    | P::Or
                    | P::Xor
            )
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `elsif` / `unless` / `while` /
// `until` condition, a statement modifier's condition, a ternary or
// C-style `for` condition, an operand of a logical chain (see
// `count_boolean_slot`). Perl is truthy-valued, so `if (-$x)`,
// `if ($x + 1)` and `if (my $y = $x)` are each a decision, and each
// scored 0 while the slot paid only for a fixed list of terminal kinds
// (#1526).
fn perl_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        perl_wrapper_operand,
        perl_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call argument,
// a ternary branch — scores only when a `!` / `not` proves it boolean
// (see `count_negated_operand`), which keeps `($a > 0) ? $b : -$b` at 2
// (the ternary node and the `>`).
fn perl_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        perl_wrapper_operand,
        perl_condition_scores_itself,
        conditions,
    );
}

fn perl_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        perl_count_condition(&condition, conditions);
    }
}

// Phase-2B (issues #403 / #1102): a ternary's condition is a boolean
// slot and each branch a negated operand, exactly as `java_walk_ternary`
// counts them. Without this Perl scored `$a ? !$b : !$c` as 1 (the
// `ternary_expression` node alone) against Java's 4.
//
// Slots are addressed by grammar FIELD, not by child index.
// tree-sitter-perl names the branches `true` / `false` rather than the
// C-family `consequence` / `alternative`, and all three slots are
// mandatory — Perl has no short-ternary elision.
fn perl_walk_ternary(node: &Node, conditions: &mut f64) {
    perl_count_slot(node.child_by_field_name("condition"), conditions);
    for field in ["true", "false"] {
        if let Some(branch) = node.child_by_field_name(field) {
            perl_count_negated(&branch, conditions);
        }
    }
}

// Each operand of a logical chain is a boolean slot (Fitzpatrick Rule 9,
// #403). `$a && $b || $c` is a left-nested chain, so an operand that is
// itself a chain is paid by its own operator's visit. The operands are
// the chain's children that are neither its operator token nor an
// `extra`: tree-sitter-perl names no operand field on a comparison or on
// the two-operand `unary_expression` it parses `$a and $b` as, which the
// field-less walk is what scores at all — it scored 0.
fn perl_count_chain_operands(chain: &Node, conditions: &mut f64) {
    if matches!(
        chain.kind_id().into(),
        Perl::BinaryExpression | Perl::UnaryExpression
    ) {
        for operand in chain.children().filter(is_operand) {
            perl_count_condition(&operand, conditions);
        }
    }
}

// Whether a pattern (`/re/` or `m{re}`) is one of the two shapes that
// are not a bare match the ABC walk should count on its own (#1467):
//
// - **Bound**: the pattern is the right operand of `$x =~ /re/` or
//   `$x !~ /re/`, whose operator token is already a condition. Read
//   through the grammar's `operator` field, so `$x ~~ /re/` — a
//   smartmatch, which no arm counts — still scores the match once.
// - **`split`'s delimiter**: `split /,/, $s` and `split(/,/, $s)` hand
//   the pattern to `split` as a separator; nothing is matched against
//   `$_`, so it is no condition. Only the *first* argument is a
//   delimiter: a pattern elsewhere in the list is an ordinary match
//   whose result `split` receives. The callee is identified by its
//   `function_name` bytes (grammar-dispatch §10) — the package
//   qualifier is ignored so `CORE::split` is covered too.
fn perl_pattern_is_bound_or_delimiter(pattern: &Node, code: &[u8], ancestors: Ancestors) -> bool {
    use Perl as P;

    let mut climb = ancestors.iter(pattern).map(|(ancestor, _)| ancestor);
    climb
        .next()
        .is_some_and(|parent| match parent.kind_id().into() {
            P::BinaryExpression => parent
                .child_by_field_name("operator")
                .is_some_and(|op| matches!(op.kind_id().into(), P::EQTILDE | P::BANGTILDE)),
            P::Arguments | P::Array => {
                // The first operand, not the first named child: a
                // comment is named, so `split( # sep⏎ /,/, $s)` read it
                // as the first argument (#1455).
                parent
                    .children()
                    .find(is_operand)
                    .is_some_and(|first| first.id() == pattern.id())
                    && climb
                        .next()
                        .is_some_and(|call| perl_call_is_split(&call, code))
            }
            _ => false,
        })
}

// Whether `call` is a `split` call. Every named child of the two call
// wrappers that take an argument list is the callee or an `args`
// field, so a list whose grandparent is one of them is its arguments.
fn perl_call_is_split(call: &Node, code: &[u8]) -> bool {
    matches!(
        call.kind_id().into(),
        Perl::CallExpressionWithSpacedArgs | Perl::CallExpressionWithArgsWithBrackets
    ) && call
        .children()
        .find(|child| child.kind_id() == Perl::CallExpressionWithBareword as u16)
        .and_then(|callee| callee.child_by_field_name("function_name"))
        .is_some_and(|name| code.get(name.start_byte()..name.end_byte()) == Some(b"split"))
}

fn perl_is_call_argument_parent(parent: Node) -> bool {
    use Perl as P;
    matches!(
        parent.kind_id().into(),
        P::CallExpressionWithArgsWithBrackets
            | P::CallExpressionWithSpacedArgs
            | P::CallExpressionWithSub
            | P::CallExpressionWithVariable
            | P::CallExpressionRecursive
            | P::MethodInvocation
    )
}

impl Abc for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(halstead, cyclomatic)
        // Exhaustive one-arm-per-grammar-kind dispatch table, like
        // `CppCode::compute`, which breaches only the cyclomatic limit
        // and so carries only that marker. Perl's arm list is the
        // longest of the family — tree-sitter-perl tokenises all
        // nineteen assignment operators and all six call-expression
        // wrappers separately — so
        // `halstead.effort` here is a count of distinct enum operands,
        // and the cyclomatic count is the number of node kinds the
        // grammar can hand us, neither being reasoning a reader must
        // do. Adding the guarded `<` / `>` arm for #1297 took the
        // count from 14 to 15, the statement-modifier arm for #1464
        // to 16, and the bare-match arm for #1467 to 17; each arm is
        // independent and self-describing like every other, and there
        // is no semantic boundary to split this lookup on.
        use Perl as P;

        match node.kind_id().into() {
            // Plain `=` and every compound assignment operator. The
            // grammar tokenises each operator separately, so one
            // textual `+=` produces exactly one token and there is no
            // double-counting via a wrapper.
            P::EQ
            | P::PLUSEQ
            | P::DASHEQ
            | P::STAREQ
            | P::SLASHEQ
            | P::PERCENTEQ
            | P::STARSTAREQ
            | P::DOTEQ
            | P::XEQ
            | P::AMPEQ
            | P::PIPEEQ
            | P::CARETEQ
            | P::LTLTEQ
            | P::GTGTEQ
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ
            | P::AMPDOTEQ
            | P::PIPEDOTEQ
            | P::CARETDOTEQ => {
                stats.assignments += 1.;
            }
            // Argument-bearing call wrappers always count.
            P::CallExpressionWithSpacedArgs
            | P::CallExpressionWithSub
            | P::CallExpressionWithArgsWithBrackets
            | P::CallExpressionWithVariable
            | P::CallExpressionRecursive
            | P::MethodInvocation => {
                stats.branches += 1.;
            }
            // Bareword-only call (`shift`, `time`, …) — count only
            // when this node is the outermost dispatch site. When the
            // bareword sits inside one of the wrappers above, the
            // outer node has already been counted and this child
            // would double the branch tally.
            P::CallExpressionWithBareword
                if !ancestors.parent(node).is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        P::CallExpressionWithSpacedArgs
                            | P::CallExpressionWithSub
                            | P::CallExpressionWithArgsWithBrackets
                            | P::CallExpressionWithVariable
                            | P::CallExpressionRecursive
                    )
                }) =>
            {
                stats.branches += 1.;
            }
            // Numeric, string, and pattern-match comparison operators
            // plus the spaceship / `cmp` three-way comparisons, and each
            // `elsif` / `else` clause of an `if` / `unless` chain.
            P::EQEQ
            | P::BANGEQ
            | P::LTEQ
            | P::GTEQ
            | P::LTEQGT
            | P::Eq
            | P::Ne
            | P::Lt
            | P::Gt
            | P::Le
            | P::Ge
            | P::Cmp
            | P::EQTILDE
            | P::BANGTILDE
            | P::ElseClause => {
                stats.conditions += 1.;
            }
            // Counts `<` / `>` only as the operator token of a
            // `binary_expression`, the allowlist polarity the rest of
            // the workspace uses. Ungated, a readline scored two
            // conditions: `<FH>` and `<$fh>` are
            // `standard_input_to_identifier` and
            // `standard_input_to_variable`, each a plain three-token
            // sequence whose brackets are the same bare `<` / `>` a
            // comparison uses. A `grammar.json` sweep of
            // tree-sitter-perl 1.1.2 finds them in exactly three
            // productions — `binary_expression` plus those two — so the
            // gate is closed (`.claude/rules/grammar-dispatch.md` §1).
            //
            // `<STDIN>` is *not* among them: the grammar lexes it as a
            // single `standard_input` token, which is why #1297's sweep
            // measured Perl at 0 and wrongly cleared it. The filehandle
            // and lexical-handle forms are the reachable ones.
            // `<=` / `>=`, the spaceship `<=>` and the word-form `lt` /
            // `gt` are distinct tokens counted above, and a heredoc
            // opener is its own token, so none reaches this arm.
            P::LT | P::GT if ancestors.parent_has_kind(node, P::BinaryExpression as u16) => {
                stats.conditions += 1.;
            }
            // A bare `/re/` or `m{re}` (sibling kinds, not aliases)
            // matches the implicit `$_`: a relational operator with no
            // operator token, so it scores by use wherever it is
            // written (#1467) — `my $r = /^#/` levels with
            // `my $r = ($x =~ /^#/)`. `perl_condition_scores_itself`
            // lists it, or every slot would score it a second time.
            // `s///` and `tr///` stay out: they edit
            // `$_` and yield a count, a policy question left to #1475.
            P::PatternMatcher | P::PatternMatcherM
                if !perl_pattern_is_bound_or_delimiter(node, code, ancestors) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each operand of a Perl
            // short-circuit / low-precedence logical chain is one
            // condition (issue #403). Covers `&&`, `||`, `//`,
            // `and`, `or`, `xor`.
            P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH | P::And | P::Or | P::Xor => {
                if let Some(chain) = ancestors.parent(node) {
                    perl_count_chain_operands(&chain, &mut stats.conditions);
                }
            }
            // An `elsif` is Java's `else if`: the `else` (+1, Rule 5) and
            // an `if` predicate slot, as Ruby's `elsif` is. It paid only
            // the first, so `elsif ($b)` scored one below
            // `elsif ($x > 1)` (#1526).
            P::ElsifClause => {
                stats.conditions += 1.;
                perl_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // `return value` names no field; its value is its only
            // operand.
            P::ReturnExpression => {
                if let Some(value) = wrapped_operand(node) {
                    perl_count_negated(&value, &mut stats.conditions);
                }
            }
            // `call(!$a, !$b)` — argument list walker. Perl wraps
            // call-argument lists in an `Array` node (same kind name
            // as the `(...)` wrapper around `if` / `while`
            // conditions). To avoid re-handling condition slots that
            // were already walked as slots, only dispatch when the
            // parent is a call-expression form. Each argument is a
            // negated operand.
            P::Array
                if ancestors
                    .parent(node)
                    .is_some_and(perl_is_call_argument_parent) =>
            {
                for argument in node.children().filter(is_operand) {
                    perl_count_negated(&argument, &mut stats.conditions);
                }
            }
            // `$a ? !$b : !$c`. Unlike the C family, this dispatcher
            // has no `?`-token arm — the grammar emits the token, but
            // the `ternary_expression` node is what carries the
            // condition tally's +1 — so this arm keeps that increment
            // and adds the three operand slots (issue #1102).
            P::TernaryExpression => {
                stats.conditions += 1.;
                perl_walk_ternary(node, &mut stats.conditions);
            }
            // Phase-2B (issue #403): condition slots, each read by the
            // grammar's `condition` field, not at child(1): a comment
            // may sit before the slot (`if # c⏎ ($b)`), which the fixed
            // index scored zero (#1455). The block forms wrap it in the
            // `Array` `(...)` shape the peel reads; the C-style `for`
            // header's condition (#1276) is bare, and `for (;;)` has
            // no condition field and counts nothing.
            //
            // Statement modifiers — `return 1 if $x;`, `next unless $ok;`
            // (issue #1464). Each is its own node whose `condition` field
            // is the predicate: a `parenthesized_argument`, or a bare
            // `arguments` list the peel reads like a `(...)` one. Perl's
            // cyclomatic dispatcher already counts these kinds, so
            // before #1464 `return 1 if $x;` scored zero conditions
            // against `if ($x) { return 1; }`'s one.
            //
            // `for_simple_statement` is deliberately absent: `print $_
            // for @list;` iterates a list and has no boolean test, and
            // the grammar names its field `list`, not `condition` —
            // mirroring the block forms, where the `foreach` shape
            // `ForStatement2` contributes nothing. `when_simple_statement`
            // is listed for parity with the cyclomatic dispatcher, but
            // only error recovery reaches it: `when` is never a modifier,
            // and `perl -c` rejects `print 6 when $x;` (grammar-dispatch
            // §6). `_if_simple` (`Perl::IfSimple`) is a hidden rule the
            // parser inlines and gets no arm (§2).
            P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::WhenSimpleStatement => {
                perl_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            _ => {}
        }
    }
}
