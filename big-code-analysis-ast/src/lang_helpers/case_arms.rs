//! Which arm of a `switch` / `match` / `case` is a decision, and which
//! is the fallback.
//!
//! Every language with a multi-way branch spells a catch-all arm —
//! `default:`, `_ =>`, `else ->`, `*)`, `case _:` — that pays no
//! decision: it is the path taken when no arm matched, the C-family
//! `default:` convention (lesson 11, #469). Some grammars give the
//! fallback a kind of its own; the rest spell it as an ordinary arm
//! whose pattern is a bare wildcard, which only the pattern's shape or
//! bytes can tell. These predicates answer that for the `CaseArm` rows
//! (`roles/<lang>.rs`, #1695) and for every metric that counts arms,
//! so no two readers disagree about which arms pay (grammar-dispatch
//! §7). They were `src/metrics/npa/shared.rs` and
//! `src/metrics/cyclomatic.rs` helpers in the root until the rows
//! gated on them.
//!
//! Each language's `default:`-equivalent has its own shape, so there is
//! one predicate per language rather than one rule: Rust and Python
//! inspect a bare `_` token, C# a `discard` or `var _`, Ruby an
//! `identifier` spelled `_`, Bash a sole `*` value, Kotlin the absence
//! of a `condition` field, Tcl and iRules an arm named `default`. A
//! guarded wildcard (`_ if g`, `case _ if g:`, `_ when g =>`, `in _ if
//! g`) still counts: the guard is a decision of its own.

use crate::node::MemoQuestion;
use crate::{Ancestors, Csharp, Irules, Node, Python, Ruby, Tcl};

/// Whether `pat` holds exactly one `underscore_id` token and no other
/// named child. Anonymous tokens such as a leading `|` in a Rust
/// or-pattern (`| _ => …`) are skipped — they do not change the
/// pattern's meaning.
///
/// Shared by the languages whose `default:`-equivalent wildcard is a
/// single `_`: Rust's `match_pattern` and Python's `case_pattern`.
/// Each passes its grammar's `UNDERSCORE` kind id. Guard handling is
/// the caller's: in Rust the guard is a sibling inside `match_pattern`
/// and so adds a named child here (this returns `false`); in Python it
/// is an `if_clause` sibling on the enclosing `case_clause`, which
/// [`python_case_clause_counts`] reads.
#[must_use]
pub(crate) fn pattern_is_bare_underscore(pat: &Node, underscore_id: u16) -> bool {
    let mut found_underscore = false;
    for child in pat.children() {
        if child.kind_id() == underscore_id {
            if found_underscore {
                return false;
            }
            found_underscore = true;
        } else if child.is_named() {
            return false;
        }
    }
    found_underscore
}

/// Whether a Python `case_clause` is a decision: its pattern is not a
/// bare `_`, or it carries an `if` guard (`case _ if g:`). The bare
/// wildcard without a guard is Python's `default:` and pays nothing,
/// as Rust's bare-`_` arm and Java's / C#'s `default:` do.
#[must_use]
pub(crate) fn python_case_clause_counts(node: &Node) -> bool {
    let mut bare_underscore = false;
    for child in node.children() {
        match child.kind_id().into() {
            Python::IfClause => return true,
            Python::CasePattern => {
                bare_underscore = pattern_is_bare_underscore(&child, Python::UNDERSCORE as u16);
                if !bare_underscore {
                    return true;
                }
            }
            _ => {}
        }
    }
    !bare_underscore
}

/// Whether a Ruby `in_clause` (`case … in`) is a decision: its pattern
/// is not a bare `_`, or it carries an `if` / `unless` guard. A bare
/// `in _` with no guard is the pattern match's default arm (#977).
///
/// Ruby spells the wildcard as an `identifier` whose text is `_` — no
/// dedicated token, unlike Rust and Python — so the pattern is read
/// from the bytes. The pattern is the first *named* child; the `in`
/// keyword is anonymous and the guard and body follow it.
#[must_use]
pub(crate) fn ruby_in_clause_counts(in_clause: &Node, code: &[u8]) -> bool {
    let mut pattern: Option<Node> = None;
    for child in in_clause.children() {
        match child.kind_id().into() {
            // `_guard` is the hidden supertype; `if_guard` / `unless_guard`
            // are the concrete nodes the grammar emits (lesson 34).
            Ruby::Guard | Ruby::IfGuard | Ruby::UnlessGuard => return true,
            _ if pattern.is_none() && child.is_named() => pattern = Some(child),
            _ => {}
        }
    }
    pattern.is_none_or(|pat| {
        !(pat.kind_id() == Ruby::Identifier as u16 && pat.utf8_text(code) == Some("_"))
    })
}

/// Whether a C# `switch_expression_arm` is the bare discard — `_` or
/// `var _`, the expression form's `default:` — with no `when` guard. A
/// guarded discard (`_ when g => …`) still counts.
///
/// `pattern` is a supertype tree-sitter flattens to the concrete
/// subtype, so a bare `_` surfaces as a direct `discard` child. `var _`
/// parses as a `declaration_pattern` of `implicit_type` and `discard`
/// rather than a `var_pattern`; a declared type (`int _`) is a type
/// test and still counts.
#[must_use]
pub(crate) fn csharp_switch_expression_arm_is_bare_discard(node: &Node) -> bool {
    use Csharp::*;

    fn is_bare_discard(child: &Node) -> bool {
        match child.kind_id().into() {
            Discard => true,
            DeclarationPattern => {
                let (mut saw_discard, mut saw_implicit_type) = (false, false);
                for sub in child.children().filter(Node::is_named) {
                    match sub.kind_id().into() {
                        Discard => saw_discard = true,
                        ImplicitType => saw_implicit_type = true,
                        _ => return false,
                    }
                }
                saw_discard && saw_implicit_type
            }
            _ => false,
        }
    }

    let mut named = node.children().filter(Node::is_named);
    let Some(pattern) = named.next() else {
        return false;
    };
    is_bare_discard(&pattern) && !named.any(|c| c.kind_id() == WhenClause as u16)
}

/// Whether a C# `case` keyword is a `switch` arm's rather than the one
/// inside `goto case 2;`, which spells the same token from a second
/// production and is an unconditional jump (#1450). `goto default;`
/// needs no twin: `default` is a distinct token no arm counts.
#[must_use]
pub(crate) fn csharp_case_token_is_switch_arm<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Csharp::SwitchSection as u16)
}

/// Whether a Kotlin `when_entry` is the `else -> …` arm. The grammar
/// attaches a `condition` field to every case-style entry; the `else`
/// arm has only its anonymous keyword child.
#[must_use]
pub(crate) fn kotlin_when_entry_is_else(node: &Node) -> bool {
    node.child_by_field_name("condition").is_none()
}

/// Whether a Bash `case_item` is the `*)` catch-all: exactly one
/// `value` field whose bytes are `*`. A multi-value pattern (`a|b`,
/// `*|b`) is not bare and still counts.
///
/// tree-sitter-bash attaches `value` to each alternation, so the items
/// are walked with one cursor — `field_name()` names the field at the
/// cursor and `goto_next_sibling()` is `O(1)` — rather than through
/// `Node::child(i)`, which is `O(i)` per call.
#[must_use]
pub(crate) fn bash_case_item_is_bare_wildcard(node: &Node, code: &[u8]) -> bool {
    let mut cursor = node.as_tree_sitter().walk();
    if !cursor.goto_first_child() {
        return false;
    }
    let mut value_count = 0usize;
    let mut sole_value_is_star = false;
    loop {
        if cursor.field_name() == Some("value") {
            value_count += 1;
            if value_count > 1 {
                return false;
            }
            sole_value_is_star = cursor.node().utf8_text(code).is_ok_and(|s| s.trim() == "*");
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    value_count == 1 && sole_value_is_star
}

/// The arm list of a Tcl `switch` written in the brace-list form:
/// the sole `braced_word` argument inside the command's `word_list`,
/// which leading options (`-exact`, `-glob`, `-regexp`, `-nocase`,
/// `--`) and the matched value precede and never parse as.
///
/// Tcl's `switch` is a generic `command` with no kind of its own
/// (#467, lesson 19), so the callers resolve the leading word first;
/// this is not re-checked here. The split form (`switch $x a {b} c
/// {d}`) yields several sibling `braced_word`s, one per body, and is
/// `None`: its bodies are sibling arguments rather than nested
/// commands, so there is no arm node to count. Idiomatic Tcl uses the
/// brace-list form, so only the uncommon style is under-counted.
#[must_use]
pub fn tcl_switch_arm_list<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let word_list = node
        .children()
        .find(|child| child.kind_id() == Tcl::WordList as u16)?;
    let mut braced_words = word_list
        .children()
        .filter(|child| child.kind_id() == Tcl::BracedWord as u16);
    let arm_list = braced_words.next()?;
    braced_words.next().is_none().then_some(arm_list)
}

/// Whether a Tcl `command` is a decision arm of a brace-list `switch`:
/// a child of the arm list of a `command` whose leading word spells
/// `switch`, and not named `default`. Each arm is a nested `command`
/// whose leading word is the pattern.
///
/// Whether the `word_list` holds exactly one `braced_word` — the
/// brace-list test [`tcl_switch_arm_list`] makes — is a scan of the
/// switch's arguments, asked once per arm; on a walk that lends a
/// memo it is answered once per switch, so a switch with `W` options
/// and `A` arms costs `O(A + W)` and not `O(A × W)`.
#[must_use]
pub(crate) fn tcl_command_is_switch_arm<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use crate::lang_helpers::tcl::tcl_command_name;

    if node.kind_id() != Tcl::Command as u16 {
        return false;
    }
    // Nearest first: the arm list (a `braced_word`), the `word_list` it
    // sits in, and the `switch` command itself.
    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    let (Some(arm_list), Some(word_list), Some(switch)) = (up.next(), up.next(), up.next()) else {
        return false;
    };
    if arm_list.kind_id() != Tcl::BracedWord as u16
        || word_list.kind_id() != Tcl::WordList as u16
        || switch.kind_id() != Tcl::Command as u16
        || tcl_command_name(&switch, code) != Some("switch")
    {
        return false;
    }
    tcl_word_list_holds_sole_braced_word(&word_list, ancestors)
        && node
            .child_by_field_name("name")
            .and_then(|pat| pat.utf8_text(code))
            != Some("default")
}

/// Whether a `word_list` holds exactly one `braced_word` — the
/// brace-list `switch`'s arm list — memoised per list on a walk that
/// lends a [`MemoQuestion::SwitchArmList`] memo, since every arm asks.
fn tcl_word_list_holds_sole_braced_word(
    word_list: &Node<'_>,
    ancestors: Ancestors<'_, '_>,
) -> bool {
    let memo = ancestors.node_memo(MemoQuestion::SwitchArmList);
    if let Some(answer) = memo.and_then(|memo| memo.get(word_list)) {
        return answer;
    }
    let sole = word_list
        .children()
        .filter(|child| child.kind_id() == Tcl::BracedWord as u16)
        .count()
        == 1;
    if let Some(memo) = memo {
        memo.set(word_list, sole);
    }
    sole
}

/// Whether an iRules `switch_arm` is a decision arm: a direct child of
/// a `switch` whose `pattern` is not `default`. Unlike Tcl, the iRules
/// grammar gives `switch` and its arms kinds of their own; an arm error
/// recovery strands under an `ERROR` with its `switch` lost has no
/// switch to be an arm of, and never counted.
#[must_use]
pub(crate) fn irules_switch_arm_is_decision<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    node.kind_id() == Irules::SwitchArm as u16
        && ancestors.parent_has_kind(node, Irules::Switch as u16)
        && node
            .child_by_field_name("pattern")
            .and_then(|pat| pat.utf8_text(code))
            != Some("default")
}
