//! The `ElseIf` gates the control-flow rows share across families
//! (#1697). Each is emitted per language because the kind ids differ;
//! what the gate reads is the same, and so is the reason it is read off
//! the walk's chain rather than through `Node::parent` /
//! `Node::prev_sibling`, which resolve the parent by descending from the
//! root (#1084, #1584). The per-language rationale — which grammar
//! shape makes the gate the right one — stays beside each invocation,
//! per `.claude/rules/macro-comments.md`.

/// `continues_else_clause`: an `if` whose parent is the grammar's
/// `$clause` kind — the `else if` of the C and JS families, whose
/// grammars wrap the chained `if` in an `else_clause` and put a nested
/// one (`else { if … }`) under a block.
macro_rules! else_if_parent_clause_gate {
    ($lang:ident, $clause:ident) => {
        fn continues_else_clause<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            ancestors.parent_has_kind(node, $lang::$clause as u16)
        }
    };
}

/// `follows_else_keyword`: an `if` whose previous sibling is the bare
/// `$keyword` token — the `else if` of Java, Groovy, C# and Kotlin,
/// whose grammars have no clause wrapper, so a nested `if` under
/// `else { … }` is told apart by its previous sibling being `{`. Through
/// `Ancestors::previous_sibling_has_kind`: a binary search through the
/// walk's `SiblingCache` on a wide parent, a short scan on a narrow one.
macro_rules! else_if_prev_sibling_gate {
    ($lang:ident, $keyword:ident) => {
        fn follows_else_keyword<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            ancestors.previous_sibling_has_kind(node, $lang::$keyword as u16)
        }
    };
}

pub(crate) use {else_if_parent_clause_gate, else_if_prev_sibling_gate};
