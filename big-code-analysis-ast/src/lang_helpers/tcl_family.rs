//! Tcl and iRules: which braced arguments of a command hold a *script*.
//!
//! Neither dialect's grammar marks the difference. `{a b}` and
//! `{puts hi}` parse to one kind, so the role is recoverable only from
//! the enclosing command's leading word and the argument's position in
//! its list (grammar-dispatch §9). This module is that lookup: two
//! tables of documented Tcl 8.6 signatures, and the position arithmetic
//! [`Getter::is_value_braced_word`] and [`Getter::is_braced_literal_slot`]
//! read them through.
//!
//! It lives here rather than in `getter.rs` for the reason the module
//! above gives: the classifiers consult it, and all three of them —
//! `Getter::get_op_type_with_code`, `Checker::is_string_with_code` and
//! `Alterator::keeps_children` — must agree on the same bytes.
//!
//! [`Getter::is_value_braced_word`]: crate::getter::Getter::is_value_braced_word
//! [`Getter::is_braced_literal_slot`]: crate::getter::Getter::is_braced_literal_slot

use crate::getter::{BracedWordKinds, node_text};
use crate::node::Node;

/// The core Tcl-family commands that evaluate a braced argument as a
/// *script* and that neither dialect's grammar models with a node of its
/// own (#1318).
///
/// A command the grammar *does* model — `proc`, `if`, `while`,
/// `foreach`, `catch`, `try`, `namespace`, iRules' `when`, `for`,
/// `switch` and `dict for` / `dict update` / `dict with` — needs no
/// entry: its body is a child of that construct's own node rather than
/// of a generic `command`, which
/// `Getter::generic_argument_command` already answers `None` for.
/// `for` and `switch` appear here because the *Tcl* grammar models
/// neither (#467, #1264); the iRules grammar models both, so those two
/// rows are live for one dialect and inert for the other.
///
/// Each entry is a command whose documented syntax puts a script in a
/// braced argument, paired with *which* of its arguments that is. Only
/// `eval` and `for` evaluate all of them; the other six mix the two
/// roles in one argument list, and the second column is what
/// `Getter::is_braced_literal_slot` reads to tell the halves apart
/// (#1381 review). Syntax and slot are both from the Tcl 8.6 manual
/// page named in the last column.
///
/// | command | syntax | evaluated | page |
/// | --- | --- | --- | --- |
/// | `after` | `after ms ?script …?` | all but the first — argument 0 is a millisecond count or a `cancel` / `idle` / `info` subcommand word | `after(n)` |
/// | `eval` | `eval arg ?arg …?` | all | `eval(n)` |
/// | `for` | `for start test next body` | all four | `for(n)` |
/// | `on` | `on code varList script` | the last | `try(n)` |
/// | `switch` | `switch ?options? string pattern body ?pattern body …?` | the arm bodies, or the single braced arm list | `switch(n)` |
/// | `time` | `time script ?count?` | argument 0 only | `time(n)` |
/// | `trap` | `trap pattern varList script` | the last | `try(n)` |
/// | `uplevel` | `uplevel ?level? arg ?arg …?` | all but a leading level specifier | `uplevel(n)` |
///
/// `on` and `trap` are listed because the iRules grammar models
/// `on_handler` / `trap_handler` only *under* `try` (pinned by
/// `irules_try_handler_kinds_appear_only_under_try`), and the Tcl
/// grammar models neither a `trap` nor a second `on`; written outside
/// that shape they parse as generic commands. Their last argument is
/// the handler script. The pattern and variable list before it are
/// values, which only `Getter::is_braced_literal_slot` tells apart —
/// the `{}` operator this table decides still bills them as blocks.
///
/// `after cancel {…}` and `after info {…}` are the one place the slot
/// column knowingly over-reports: those arguments identify a *pending*
/// script by its text rather than supplying one to run. Calling them
/// scripts costs nothing a reader would notice — the text is real code
/// either way — and telling them apart needs the subcommand dispatch
/// the paragraph below rules out.
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
/// incomplete — see `Getter::is_value_braced_word` for what an
/// unlisted command defaults to and why.
const SCRIPT_TAKING_COMMANDS: [(&str, ScriptSlots); 8] = [
    ("after", ScriptSlots::EveryButFirst),
    ("eval", ScriptSlots::Every),
    ("for", ScriptSlots::Every),
    ("on", ScriptSlots::Last),
    (SWITCH_COMMAND, ScriptSlots::SwitchArms),
    ("time", ScriptSlots::Only(0)),
    ("trap", ScriptSlots::Last),
    ("uplevel", ScriptSlots::EveryButLeadingLevel),
];

/// Named because two rules have to agree on it: `switch` takes a
/// script, and its *arm bodies* are scripts too even though the Tcl
/// grammar hangs them off a `command` named after the pattern
/// (`Getter::is_switch_arm`). Spelling it twice would let one drift.
pub(crate) const SWITCH_COMMAND: &str = "switch";

/// The `namespace` subcommands with a script argument, and which of
/// their arguments it is (Tcl 8.6 `namespace(n)`):
/// `namespace eval ns ?arg …?` evaluates everything after the namespace
/// name, `namespace inscope ns script ?arg …?` evaluates the second
/// argument and appends the rest to it as list elements, and
/// `namespace code script` has no name argument at all. Every
/// subcommand *not* listed takes values throughout — `export {pattern}`,
/// `path {ns …}`, `ensemble create -map {dict}` — which is the
/// [`ScriptSlots::NoneOfThem`] default [`namespace_script_slots`] falls
/// back to.
const NAMESPACE_SCRIPT_SUBCOMMANDS: [(&str, ScriptSlots); 3] = [
    ("code", ScriptSlots::Every),
    ("eval", ScriptSlots::EveryButFirst),
    ("inscope", ScriptSlots::Only(1)),
];

/// The `switch` options that consume the word after them, so the
/// subject scan must step over it: `-matchvar varName` and
/// `-indexvar varName` (Tcl 8.6 `switch(n)`; both are `-regexp`-only).
/// The other four — `-exact`, `-glob`, `-regexp`, `-nocase` — are bare
/// flags and need no entry.
const SWITCH_OPTIONS_TAKING_A_WORD: [&str; 2] = ["-indexvar", "-matchvar"];

/// `switch`'s end-of-options marker: whatever follows it is the
/// subject, even a subject that itself begins with `-`.
const SWITCH_OPTION_TERMINATOR: &str = "--";

/// Which of a Tcl-family construct's arguments the interpreter
/// *evaluates*. Positions count from the construct's first argument,
/// so the subcommand of a `namespace` — which shares the one
/// `word_list` with the arguments — is not one of them.
///
/// Every position a variant does not name holds a value: a millisecond
/// count, an iteration count, a namespace name, a `switch` subject or
/// pattern, a `try` handler's error code and variable list. The two
/// tables above pair one of these with each command whose signature
/// mixes the roles, and [`fills_script_slot`] applies it.
#[derive(Clone, Copy)]
pub(crate) enum ScriptSlots {
    /// Every argument: `eval arg ?arg …?`, `for start test next body`,
    /// `namespace code script`.
    Every,
    /// No argument. The default for a `namespace` subcommand the table
    /// does not list, every one of which takes values throughout.
    NoneOfThem,
    /// Every argument but the first: `after ms ?script …?`,
    /// `namespace eval ns ?arg …?`.
    EveryButFirst,
    /// Exactly one argument, by position: `time script ?count?` is
    /// `Only(0)`, `namespace inscope ns script ?arg …?` is `Only(1)`.
    Only(usize),
    /// The last argument: `on code varList script` and
    /// `trap pattern varList script`.
    Last,
    /// Every argument but a leading level specifier:
    /// `uplevel ?level? arg ?arg …?`, where an argument 0 that does not
    /// read as a level is the start of the script rather than a value
    /// ([`reads_as_uplevel_level`]).
    EveryButLeadingLevel,
    /// The evaluated half of `switch ?options? string pattern body …`
    /// ([`switch_arm_slot`]).
    SwitchArms,
}

/// One dialect's kind ids paired with the buffer the nodes under test
/// were parsed from.
///
/// The two are a unit: `BracedWordKinds` names ids in *this* grammar and
/// `code` must be the exact source *this* node came from, the same-parse
/// precondition [`Getter`] documents. Threading them as one value keeps
/// every rule below at one context parameter and, as with the kind table
/// itself, stops a caller pairing a node with the wrong buffer.
///
/// [`Getter`]: crate::getter::Getter
#[derive(Clone, Copy)]
pub(crate) struct Dialect<'a> {
    /// The source `word` and its siblings were parsed from.
    pub(crate) code: &'a [u8],
    /// This grammar's braced-word kind ids.
    pub(crate) kinds: &'a BracedWordKinds,
}

/// The [`ScriptSlots`] of a generic command named `command`, or `None`
/// when no core command of that name evaluates a braced argument.
pub(crate) fn script_slots(command: &str) -> Option<ScriptSlots> {
    SCRIPT_TAKING_COMMANDS
        .iter()
        .find_map(|&(name, slots)| (name == command).then_some(slots))
}

/// The [`ScriptSlots`] of the subcommand a `namespace` construct's
/// `word_list` names — its first entry, before the arguments proper.
///
/// `None` for a subcommand that is not a plain word
/// (`namespace $sub {…}`), which is unresolvable and keeps the script
/// answer, exactly as an unresolvable command name does.
pub(crate) fn namespace_script_slots(
    word_list: &Node<'_>,
    dialect: Dialect<'_>,
) -> Option<ScriptSlots> {
    let subcommand = word_list.child(0)?;
    if subcommand.kind_id() != dialect.kinds.simple_word {
        return None;
    }
    let subcommand = node_text(dialect.code, &subcommand)?;
    Some(
        NAMESPACE_SCRIPT_SUBCOMMANDS
            .iter()
            .find_map(|&(name, slots)| (name == subcommand).then_some(slots))
            .unwrap_or(ScriptSlots::NoneOfThem),
    )
}

/// One construct's argument layout: which slots hold a script, and
/// where in the shared `word_list` its arguments begin. The two always
/// travel together and are both plain scalars, so they ride in one
/// value rather than as two same-shaped parameters.
#[derive(Clone, Copy)]
pub(crate) struct ArgumentRoles {
    /// The evaluated slots, counted from `first`.
    pub(crate) slots: ScriptSlots,
    /// The index the construct's arguments start at — `1` for a
    /// `namespace`, whose subcommand takes the slot before them, and
    /// `0` for a generic command, which keeps its name in a field of
    /// its own.
    pub(crate) first: usize,
}

/// Whether the braced `word` fills a slot of `arguments` that `roles`
/// names as evaluated.
///
/// A word whose index cannot be resolved keeps the construct-wide
/// script answer, which is what every caller had before the slot table
/// existed.
pub(crate) fn fills_script_slot(
    word: &Node<'_>,
    arguments: &Node<'_>,
    roles: ArgumentRoles,
    dialect: Dialect<'_>,
) -> bool {
    // `O(log n)` in the argument count, as in `is_switch_arm_body`, and
    // for the same reason: every braced argument of a wide one-line
    // command asks, so a sibling scan here is quadratic in the width of
    // that line (#1381 review).
    let mut cursor = arguments.cursor();
    let index = cursor.goto_first_child_for_byte(word.start_byte());
    // Unreachable from `is_braced_literal_slot`, which reaches here only
    // for a word whose own parent is `arguments`. It is not decoration:
    // `goto_first_child_for_byte` answers with a *neighbouring* child for
    // a byte that starts no child, so a caller pairing a word with
    // another command's argument list would otherwise be given that
    // neighbour's slot. `is_switch_arm_body` carries the same id check
    // for the same reason.
    let Some(index) = index.filter(|_| cursor.node().id() == word.id()) else {
        return true;
    };
    // Also unreachable, and guarding the other half of the same pairing:
    // `roles.first` is 1 only for a `namespace`, whose slot 0 holds the
    // subcommand — and `namespace_script_slots` resolves a layout at all
    // only when that slot is a `simple_word`, which a braced word is not.
    // Loosen that guard and this is what stops slot 0 reading as the
    // argument before the first.
    let Some(position) = index.checked_sub(roles.first) else {
        return true;
    };
    match roles.slots {
        ScriptSlots::Every => true,
        ScriptSlots::NoneOfThem => false,
        ScriptSlots::EveryButFirst => position > 0,
        ScriptSlots::Only(evaluated) => position == evaluated,
        ScriptSlots::Last => position + 1 == arguments.child_count() - roles.first,
        ScriptSlots::EveryButLeadingLevel => {
            position > 0 || !reads_as_uplevel_level(word, dialect.code)
        }
        // `switch` is a generic command, so `roles.first` is 0 and the
        // absolute index is the position; the subject scan counts from
        // the same origin, so it takes the index.
        ScriptSlots::SwitchArms => switch_arm_slot(arguments, index, dialect),
    }
}

/// Whether a braced argument 0 of `uplevel` reads as a level specifier
/// rather than as the first word of the script.
///
/// `uplevel` decides this from the argument's *value*, so the test is
/// on the text between the braces: an optional `#` prefix and then an
/// integer, whitespace-tolerant because `Tcl_GetInt` is (Tcl 8.6
/// `uplevel(n)`). `uplevel {set x 2}` has no level and must stay whole.
///
/// Unreadable text and text that is not brace-delimited share one exit
/// because neither is a level and the caller cannot tell them apart
/// anyway: `node_text` answers `None` for non-UTF-8 bytes, which
/// `Ast::parse` accepts, and a caller reaching this with a node that is
/// not a `braced_word` has no level either.
fn reads_as_uplevel_level(word: &Node<'_>, code: &[u8]) -> bool {
    let Some(inner) = node_text(code, word)
        .and_then(|text| text.strip_prefix('{'))
        .and_then(|inner| inner.strip_suffix('}'))
    else {
        return false;
    };
    let inner = inner.trim();
    let digits = inner.strip_prefix('#').map_or(inner, str::trim_start);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether the argument at `index` of a `switch` command is one of the
/// arms it evaluates, rather than an option, an option's operand, the
/// subject, or a pattern.
fn switch_arm_slot(arguments: &Node<'_>, index: usize, dialect: Dialect<'_>) -> bool {
    let Some(subject) = switch_subject_index(arguments, dialect) else {
        // No subject: every word was a leading option or an option's
        // operand, as in the truncated `switch -matchvar {m}`. Nothing
        // says which half of the list this word is in, so keep the
        // construct-wide script answer.
        return true;
    };
    if index <= subject {
        return false;
    }
    // `switch ?options? string {pattern body ?pattern body …?}`: when
    // exactly one argument follows the subject it is the braced arm
    // list, a script whose interior `is_switch_arm` classifies.
    if arguments.child_count() == subject + 2 {
        return true;
    }
    // The flat spelling runs `pattern body pattern body …` from the
    // subject, paired by index with no marker on either half, so an
    // even offset from it is a body — the same parity
    // `is_switch_arm_body` reads inside a braced arm list.
    (index - subject).is_multiple_of(2)
}

/// The index of a `switch` command's subject among `arguments`: the
/// first word that is neither a leading option nor an option's operand,
/// or the word after a `--` terminator.
///
/// The scan stops at the first argument that is not a plain option
/// word, and a braced word never is one, so it ends at or before
/// whichever braced word the caller is asking about — its length is the
/// leading-option count, not the argument count. Every braced argument
/// of a wide one-line arm list asks, so an unbounded scan here would be
/// the quadratic `is_switch_arm_body` documents (#1381 review).
fn switch_subject_index(arguments: &Node<'_>, dialect: Dialect<'_>) -> Option<usize> {
    let mut skip_operand = false;
    for (index, child) in arguments.children().enumerate() {
        if skip_operand {
            skip_operand = false;
            continue;
        }
        let Some(word) = (child.kind_id() == dialect.kinds.simple_word)
            .then(|| node_text(dialect.code, &child))
            .flatten()
        else {
            return Some(index);
        };
        if word == SWITCH_OPTION_TERMINATOR {
            return Some(index + 1);
        }
        if !word.starts_with('-') {
            return Some(index);
        }
        skip_operand = SWITCH_OPTIONS_TAKING_A_WORD.contains(&word);
    }
    None
}

#[cfg(test)]
#[cfg(feature = "tcl")]
mod tests {
    use super::{
        ArgumentRoles, Dialect, ScriptSlots, fills_script_slot, node_text, reads_as_uplevel_level,
    };
    use crate::Tcl;
    use crate::lang_helpers::tcl::BRACED_WORD_KINDS;
    use crate::node::{Node, Tree};

    /// Every `braced_word` in `code`, in source order.
    fn braced_words<'a>(root: &Node<'a>, out: &mut Vec<Node<'a>>) {
        if root.kind_id() == Tcl::BracedWord as u16 {
            out.push(*root);
        }
        for child in root.children() {
            braced_words(&child, out);
        }
    }

    /// The first `word_list` in `code`, in source order.
    fn first_word_list<'a>(root: &Node<'a>) -> Option<Node<'a>> {
        if root.kind_id() == Tcl::WordList as u16 {
            return Some(*root);
        }
        root.children().find_map(|child| first_word_list(&child))
    }

    /// `fills_script_slot` keeps the construct-wide *script* answer for a
    /// word it cannot place among the arguments it was handed.
    ///
    /// Neither pairing arises from `is_braced_literal_slot`, which only
    /// ever asks about a word whose own parent is the argument list. Both
    /// guards are still load-bearing, and the comments at each say why:
    /// `goto_first_child_for_byte` answers with a *neighbouring* child for
    /// a byte that starts no child, so without the id check a nested word
    /// silently borrows its enclosing sibling's slot; and without the
    /// `checked_sub` a `namespace`'s slot 0 would read as the argument
    /// before its first.
    ///
    /// The unplaceable word is a *nested* one — `{b c}` inside
    /// `{a {b c}}` — rather than one from another command, because a word
    /// outside the list's byte range makes the cursor answer `None` and
    /// never reaches the id check at all. Both rows assert `true` (script)
    /// against a fixture whose placeable word answers `false`, so neither
    /// is the constant either guard could be replaced by.
    #[test]
    fn an_unplaceable_word_keeps_the_script_answer() {
        // `namespace {export} {a {b c}}`: a braced subcommand in slot 0 —
        // the shape `namespace_script_slots` refuses, reconstructed here
        // with the layout it would otherwise have produced — and a nested
        // `{b c}` whose bytes fall inside slot 1 without being slot 1.
        let code = b"namespace {export} {a {b c}}\n";
        let tree = Tree::new::<crate::langs::TclCode>(code);
        let root = tree.get_root();
        let dialect = Dialect {
            code,
            kinds: &BRACED_WORD_KINDS,
        };
        let mut words = Vec::new();
        braced_words(&root, &mut words);
        let texts: Vec<&str> = words
            .iter()
            .map(|word| node_text(code, word).expect("ascii fixture"))
            .collect();
        assert_eq!(
            texts,
            ["{export}", "{a {b c}}", "{b c}"],
            "the fixture must hold the subcommand, slot 1, and a word \
             nested inside slot 1"
        );
        let arguments = first_word_list(&root).expect("the namespace has an argument list");
        let roles = ArgumentRoles {
            slots: ScriptSlots::NoneOfThem,
            first: 1,
        };

        // `{export}` sits *before* the first argument.
        assert!(
            fills_script_slot(&words[0], &arguments, roles, dialect),
            "a word before the construct's first argument keeps the script answer"
        );
        // `{a {b c}}` is slot 1 — placeable, and `NoneOfThem` calls it a value.
        assert!(
            !fills_script_slot(&words[1], &arguments, roles, dialect),
            "the fixture's placeable word must answer the other way, or the \
             rows around it are asserting a constant"
        );
        // `{b c}` starts inside slot 1 but is not slot 1.
        assert!(
            fills_script_slot(&words[2], &arguments, roles, dialect),
            "a word that is not one of these arguments keeps the script \
             answer, rather than borrowing the slot it is nested in"
        );
    }

    /// `reads_as_uplevel_level` accepts what `Tcl_GetInt` accepts, and
    /// nothing else (Tcl 8.6 `uplevel(n)`).
    ///
    /// The last row is the shared exit for text that is not a readable
    /// `{…}`: a `simple_word` has no braces, and neither has a node whose
    /// bytes are not UTF-8 — `Ast::parse` accepts those and `node_text`
    /// answers `None` for them. Both mean "not a level" and no caller can
    /// tell them apart, which is why they share one line.
    ///
    /// The `#` half of the rule has no row because neither pinned grammar
    /// can spell it: `#` at a command position opens a comment, so `{#0}`
    /// parses as a brace block holding a comment that swallows the rest of
    /// the line, and the enclosing command lands under an `ERROR` that
    /// `is_braced_literal_slot` rejects before the slot table. Real Tcl
    /// accepts `uplevel {#0} {…}`, so the prefix stays in the rule against
    /// a grammar that learns to parse it.
    #[test]
    fn a_level_specifier_is_an_integer_and_nothing_else() {
        let code = b"uplevel {1} { 2 } {a} {} {1x} plain\n";
        let tree = Tree::new::<crate::langs::TclCode>(code);
        let root = tree.get_root();
        let mut words = Vec::new();
        braced_words(&root, &mut words);
        let read: Vec<bool> = words
            .iter()
            .map(|word| reads_as_uplevel_level(word, code))
            .collect();
        assert_eq!(
            read,
            [true, true, false, false, false],
            "`{{1}}` and `{{ 2 }}` are levels; a word, an empty word and \
             `1x` are not"
        );

        let arguments = first_word_list(&root).expect("the command has an argument list");
        let plain = arguments
            .children()
            .find(|node| node.kind_id() == Tcl::SimpleWord as u16)
            .expect("the fixture ends in a bare word");
        assert!(
            !reads_as_uplevel_level(&plain, code),
            "a word that is not brace-delimited carries no level"
        );
    }
}
