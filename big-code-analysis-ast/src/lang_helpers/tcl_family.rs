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

use crate::documentation::budget::WorkBudget;
use crate::documentation::byte_scan;
use crate::getter::{BracedWordKinds, node_text};
use crate::node::{Ancestors, Cursor, MemoQuestion, Node};
use std::cell::Cell;

// The sibling scan `argument_slots_are_readable` runs, counted so a
// test can see that a walk lending a `FoldCache` runs it once per
// argument list rather than once per braced word: the answers are
// identical either way, and only the count tells the two apart (#1686).
// Readable from the root crate's tests through `test-support`, since
// the walks that lend the cache live there.
crate::observation::counter!(
    argument_slot_scans,
    cfg(any(test, feature = "test-support"))
);

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
/// | `on` | `on code varList script` | the last of exactly three | `try(n)` |
/// | `switch` | `switch ?options? string pattern body ?pattern body …?` | the arm bodies, or the single braced arm list | `switch(n)` |
/// | `time` | `time script ?count?` | argument 0 only | `time(n)` |
/// | `trap` | `trap pattern varList script` | the last of exactly three | `try(n)` |
/// | `uplevel` | `uplevel ?level? arg ?arg …?` | all but a leading level specifier | `uplevel(n)` |
///
/// `on` and `trap` are listed because the iRules grammar models
/// `on_handler` / `trap_handler` only *under* `try` (pinned by
/// `irules_try_handler_kinds_appear_only_under_try`), and the Tcl
/// grammar models neither a `trap` nor a second `on`; written outside
/// that shape they parse as generic commands. Their last argument is
/// the handler script. The pattern and variable list before it are
/// values, which only `Getter::is_braced_literal_slot` tells apart —
/// and since #1382 every classifier reads that answer, so their braces
/// quote rather than open. Both rows carry the clause's arity for the
/// reason [`ScriptSlots::LastOf`] gives: these are the two commands a
/// grammar that models neither can group a *following* clause into.
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
    ("on", ScriptSlots::LastOf(HANDLER_CLAUSE_ARITY)),
    (SWITCH_COMMAND, ScriptSlots::SwitchArms),
    ("time", ScriptSlots::Only(0)),
    ("trap", ScriptSlots::LastOf(HANDLER_CLAUSE_ARITY)),
    ("uplevel", ScriptSlots::EveryButLeadingLevel),
];

/// `on code varList script` and `trap pattern varList script` each take
/// exactly three arguments (Tcl 8.6 `try(n)`). See
/// [`ScriptSlots::LastOf`] for why the count is part of the rule rather
/// than a comment on it.
const HANDLER_CLAUSE_ARITY: usize = 3;

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
    /// The last argument of a list of exactly `n`:
    /// `on code varList script` and `trap pattern varList script` are
    /// both `LastOf(3)`.
    ///
    /// The arity is the rule, not a note beside it. Neither command is
    /// modelled outside a `try`, so a top-level
    /// `try {…} trap {p} {v} {b} finally {c}` parses the whole tail as
    /// *one* generic `trap` command with five arguments —
    /// `p v b finally c`. "The last one is the script" then names the
    /// `finally` body and calls the handler body `b` a value, which
    /// withdrew the `{}` of a block the source does contain, reported
    /// it from `bca find --type string`, and flattened it out of the
    /// `Ast` dump: the #1381 defect, reopened for one shape (#1382
    /// review). An argument count the signature does not admit means
    /// the grammar grouped something else in, so no position in the
    /// list is trustworthy and every argument keeps the construct-wide
    /// script answer — the same answer [`argument_slots_are_readable`]
    /// gives for the other way a layout becomes unreadable.
    LastOf(usize),
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

/// A leading word with its global-namespace qualifier removed.
///
/// `::switch` *is* `switch`: a leading `::` names the global namespace,
/// and inside a `namespace eval` body it is the spelling that guarantees
/// the core command rather than a local proc shadowing it. Any rule that
/// resolves a leading word against a table of core command names has to
/// normalise through here, or the score moves with how the author spelled
/// a command that resolves identically. Four metrics read the leading
/// word without the slot table — cognitive, cyclomatic, ABC and nexits —
/// and each scored the qualified spelling as an ordinary call until this
/// was shared (#1381 review).
///
/// Only the *leading* qualifier is stripped. `ns::eval` is a different
/// command living in `ns` and must not be mistaken for the core one;
/// `BRACED_WORD_VALUE_CASES` in `src/metrics/halstead.rs` pins that
/// direction, and stripping every `::` segment would promote it.
#[inline]
#[must_use]
pub fn strip_global_qualifier(name: &str) -> &str {
    name.strip_prefix("::").unwrap_or(name)
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

/// Whether `arguments`' own slot sequence can be read positionally — the
/// precondition every rule below rests on (#1381 review).
///
/// The obvious spelling, `owner.has_error()`, is wrong and was measured
/// wrong: [`Node::has_error`] is transitive, so a stray `]` buried in one
/// argument's *script* withdrew the layout from every sibling.
/// `after {100} {puts ]}` lost `{100}`, and the dump then rendered it as
/// a fabricated `command` named `100` — the defect the slot table exists
/// to remove. An error *inside* an argument moves no sibling's index; an
/// `ERROR` or MISSING token holding a slot of its own moves every index
/// after it, and that is the only shape that makes a position
/// meaningless.
///
/// `has_error` stays as the `O(1)` pre-filter, so a well-formed command —
/// every command in a file that parses — never pays for the scan, and
/// the scan is bounded by the argument count when it does.
///
/// Every braced word of `arguments` asks, so under a walk that lent a
/// [`FoldCache`](crate::FoldCache) the scan's answer is memoised by the
/// list's id and runs once per list; unmemoised, a wide one-line
/// `eval` carrying one stray `]` was quadratic in its width — 4 s for
/// 16 000 words — on every walk that reaches a braced word's role
/// (#1686). `ancestors` is the *word's* chain, which is the one the
/// walk lent its cache to; a chain derived through [`Ancestors::iter`]
/// carries the same cache. The answer depends on the tree alone, so
/// pre- and post-order walks store the same one.
///
/// [`Node::has_error`]: crate::Node::has_error
pub(crate) fn argument_slots_are_readable(
    owner: &Node<'_>,
    arguments: &Node<'_>,
    ancestors: Ancestors<'_, '_>,
) -> bool {
    if !owner.has_error() {
        return true;
    }
    ancestors.memoised(MemoQuestion::ArgumentSlotsReadable, arguments, || {
        argument_slot_scans::record();
        !arguments
            .children()
            .any(|slot| slot.is_error() || slot.is_missing())
    })
}

/// Whether the braced `word` fills a slot of `arguments` that `roles`
/// names as evaluated.
///
/// A word whose index cannot be resolved keeps the construct-wide
/// script answer, which is what every caller had before the slot table
/// existed.
pub(crate) fn fills_script_slot<'t>(
    word: &Node<'t>,
    arguments: &Node<'t>,
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
        // `checked_sub` for the same reason the one above it has one:
        // `LastOf` is reachable only from a table row paired with
        // `first == 0` today, and such a row added to
        // `NAMESPACE_SCRIPT_SUBCOMMANDS` would pair it with `1` and
        // underflow — a debug panic, or a release wrap to `usize::MAX`
        // that answers `false` for every argument. `None` keeps the
        // construct-wide script answer, as every other unresolvable
        // position here does — and so does a count the signature does
        // not admit, which is the whole of the arity rule.
        ScriptSlots::LastOf(arity) => arguments
            .child_count()
            .checked_sub(roles.first)
            .is_none_or(|count| count != arity || position + 1 == count),
        ScriptSlots::EveryButLeadingLevel => {
            position > 0 || !reads_as_uplevel_level(word, dialect.code)
        }
        // `switch` is a generic command, so `roles.first` is 0 and the
        // absolute index is the position; the subject scan counts from
        // the same origin, so it takes the index. The cursor is handed on
        // rather than rebuilt: `Cursor::new` heap-allocates its stack, and
        // every braced argument of a one-line arm list reaches here.
        ScriptSlots::SwitchArms => switch_arm_slot(arguments, index, dialect, &mut cursor),
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
fn switch_arm_slot<'t>(
    arguments: &Node<'t>,
    index: usize,
    dialect: Dialect<'_>,
    cursor: &mut Cursor<'t>,
) -> bool {
    let Some(subject) = switch_subject_index(arguments, dialect, cursor) else {
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
/// the quadratic `is_switch_arm_body` documents (#1381 review). It runs
/// over the caller's cursor for the same reason.
///
/// # The option list is resolved statically, and Tcl resolves it at run time
///
/// A word that is not a `simple_word` is taken to be the subject. That
/// is right for the overwhelmingly common `switch $v {…}` and for a
/// braced subject, and wrong for a legal but vanishingly rare computed
/// *option*: Tcl substitutes each word before dispatching, so
/// `switch "-exact" $v a {…}` and `switch $opt $v a {…}` really do pass
/// an option there. Taken as the subject, every later index shifts by
/// one and the pattern/body parity below inverts — the bodies read as
/// literals and the patterns as scripts. The same static-resolution
/// limit `command_leading_word` documents for a command *name*, and
/// answered the same way: guessing keeps the common form right, where
/// returning `None` would surrender `switch $v` to the construct-wide
/// script answer.
fn switch_subject_index<'t>(
    arguments: &Node<'t>,
    dialect: Dialect<'_>,
    cursor: &mut Cursor<'t>,
) -> Option<usize> {
    let mut skip_operand = false;
    for (index, child) in arguments.children_with(cursor).enumerate() {
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

/// The kind ids a reader of a script's comments and lines needs. Both
/// dialects spell a comment `#[^\n]*` and end a command with a line-feed
/// token, under ids of their own.
#[derive(Clone, Copy)]
pub(crate) struct BodyRowKinds {
    /// `comment`.
    pub(crate) comment: u16,
    /// The line-feed terminator. One token can cover several line ends.
    pub(crate) line_feed: u16,
}

/// How a dialect reads one comment that starts a row of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowMark {
    /// Documentation while a block is open, and otherwise nothing.
    Text,
    /// Opens a block wherever it stands, and is its first row.
    Opens,
    /// No text and no boundary: a block neither starts nor ends at it.
    Silent,
}

/// What one direct child of a script body is to the body's documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyRow {
    /// Not documentation: a line end, a command, a comment outside every block.
    Outside,
    /// A row of a block. Its spelling ends at this byte.
    Member(usize),
    /// A [`RowMark::Silent`] row. Its spelling ends at this byte.
    Directive(usize),
    /// A recovery node that may hold documentation rows.
    Obscured,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    // No command and no documentation row yet: blank rows do not matter.
    Leading,
    Open,
    Closed,
}

// A child that is no line end, as the child after it sees it.
#[derive(Clone, Copy)]
struct Line {
    start: usize,
    spelling_end: usize,
    end: usize,
    first_row: usize,
    last_row: usize,
    continues: bool,
    member: bool,
}

/// Reads the direct children of a script body, in order, for its lead
/// block and for the later blocks a dialect's [`RowMark`] opens.
///
/// The lead block is the first run of comment rows. Blank rows may precede
/// it; a blank row or a command ends it. Only a child that is a `comment`
/// can be a row, so a comment row nested in a command is not read. Where a
/// comment stands is read from the bytes between it and the child before
/// it, because a line that continues shows in no node.
pub(crate) struct BodyRows<'a, B> {
    code: &'a [u8],
    kinds: BodyRowKinds,
    budget: &'a B,
    block: Block,
    above: Option<Line>,
}

impl<'a, B: WorkBudget> BodyRows<'a, B> {
    /// A reader standing after the opening brace of a body.
    pub(crate) fn new(code: &'a [u8], kinds: BodyRowKinds, budget: &'a B) -> Self {
        Self {
            code,
            kinds,
            budget,
            block: Block::Leading,
            above: None,
        }
    }

    /// Classifies the next child between the braces. `mark` is asked at
    /// most once, for a comment that starts a row of its own.
    pub(crate) fn read(
        &mut self,
        child: tree_sitter::Node<'_>,
        mark: impl FnOnce(&[u8], &B) -> Result<RowMark, B::Error>,
    ) -> Result<BodyRow, B::Error> {
        // A zero-width token is one the parser invented while recovering.
        let real = child.start_byte() < child.end_byte();
        if real && child.kind_id() == self.kinds.line_feed {
            return Ok(BodyRow::Outside);
        }
        let here = self.line(child)?;
        let above = self.above.replace(here);
        let comment = real && child.kind_id() == self.kinds.comment;
        // A comment runs to its line end. One that stops before it was cut
        // short by the parser, at a NUL byte.
        let cut = comment && self.code.get(here.end).is_some_and(|&next| next != b'\n');
        if comment && !cut {
            return self.comment(here, above, mark);
        }
        Ok(self.ends_block(here, above, cut || child.is_error()))
    }

    // What the child after this one needs to know of it.
    fn line(&self, child: tree_sitter::Node<'_>) -> Result<Line, B::Error> {
        let start = child.start_byte();
        let spelling_end = comment_spelling_end(self.code, start, child.end_byte());
        Ok(Line {
            start,
            spelling_end,
            end: child.end_byte(),
            first_row: child.start_position().row,
            last_row: child.end_position().row,
            continues: line_continues(&self.code[start..spelling_end], self.budget)?,
            member: false,
        })
    }

    // A command, a `;` or a recovery node ends the block. A recovery node
    // may hold rows that the parser did not read as comments: the next rows
    // of the block when it starts a row with `#`, and a row that opens a
    // block, wherever it stands, when it reaches a later row. The rows are
    // physical here: a continued row counts as a row, which can only
    // withhold more.
    fn ends_block(&mut self, here: Line, above: Option<Line>, recovery: bool) -> BodyRow {
        self.block = Block::Closed;
        let own_row = above.is_none_or(|above| here.first_row > above.last_row);
        let comment = own_row && self.code.get(here.start) == Some(&b'#');
        if recovery && (comment || here.last_row > here.first_row) {
            BodyRow::Obscured
        } else {
            BodyRow::Outside
        }
    }

    fn comment(
        &mut self,
        here: Line,
        above: Option<Line>,
        mark: impl FnOnce(&[u8], &B) -> Result<RowMark, B::Error>,
    ) -> Result<BodyRow, B::Error> {
        let Some(above) = above else {
            return self.marked(here, mark);
        };
        // The line ends between the child above and this comment say where
        // the comment stands. A backslash before a line end joins the rows.
        let gap = &self.code[above.spelling_end..here.start];
        match unescaped_line_ends(gap, above.continues, 2, self.budget)? {
            // On the Tcl line of the child above: the rest of a comment, or
            // a `#` that starts no command of its own row.
            0 if above.member => Ok(self.member(here.spelling_end)),
            0 => Ok(BodyRow::Outside),
            1 => self.marked(here, mark),
            // A blank row lies between.
            _ => {
                if self.block == Block::Open {
                    self.block = Block::Closed;
                }
                self.marked(here, mark)
            }
        }
    }

    fn marked(
        &mut self,
        here: Line,
        mark: impl FnOnce(&[u8], &B) -> Result<RowMark, B::Error>,
    ) -> Result<BodyRow, B::Error> {
        Ok(
            match mark(&self.code[here.start..here.spelling_end], self.budget)? {
                RowMark::Silent => BodyRow::Directive(here.spelling_end),
                RowMark::Text if self.block == Block::Closed => BodyRow::Outside,
                RowMark::Opens | RowMark::Text => self.member(here.spelling_end),
            },
        )
    }

    fn member(&mut self, spelling_end: usize) -> BodyRow {
        self.block = Block::Open;
        if let Some(above) = &mut self.above {
            above.member = true;
        }
        BodyRow::Member(spelling_end)
    }
}

/// The end of the spelling of a node that ends its row, as a `comment`
/// does: the node's end, without a carriage return that a line feed follows.
/// Both grammars put the CR of a CRLF into the comment token.
///
/// The bytes `start..=end` must be included in the parse.
pub(crate) fn comment_spelling_end(code: &[u8], start: usize, end: usize) -> usize {
    if end > start && code.get(end) == Some(&b'\n') && code.get(end - 1) == Some(&b'\r') {
        end - 1
    } else {
        end
    }
}

/// Whether a line with this spelling continues on the next row: it ends in
/// an odd run of backslashes. An even run is escaped backslashes. The same
/// reading as `continuing_backslash` in the root crate's LOC metric, which
/// this crate cannot call.
pub(crate) fn line_continues<B: WorkBudget>(spelling: &[u8], budget: &B) -> Result<bool, B::Error> {
    let run = match byte_scan::rposition(spelling, |byte| byte != b'\\', budget)? {
        Some(last) => spelling.len() - 1 - last,
        None => spelling.len(),
    };
    Ok(run % 2 == 1)
}

/// Counts the line ends in `gap` that no backslash escapes, and stops at
/// `limit`. `gap` is the bytes between two nodes of one script level: white
/// space, line ends and backslash-newline pairs. `escaped` says whether the
/// node before the gap ends in an odd run of backslashes.
///
/// A line end is what Tcl takes for one when it reads a script file: LF,
/// CR LF, or a CR alone. The count says how Tcl reads the node after the
/// gap: 0, it stands on the line of the node before it; 1, it starts the
/// next row; 2, a blank row lies between.
pub(crate) fn unescaped_line_ends<B: WorkBudget>(
    gap: &[u8],
    escaped: bool,
    limit: usize,
    budget: &B,
) -> Result<usize, B::Error> {
    let escaped = Cell::new(escaped);
    let after_cr = Cell::new(false);
    let found = Cell::new(0);
    byte_scan::position(
        gap,
        |byte| {
            let lf_of_crlf = after_cr.replace(byte == b'\r') && byte == b'\n';
            match byte {
                b'\\' => escaped.set(!escaped.get()),
                // Counted, or escaped, with its carriage return.
                b'\n' if lf_of_crlf => {}
                b'\n' | b'\r' => found.set(found.get() + usize::from(!escaped.replace(false))),
                _ => escaped.set(false),
            }
            found.get() == limit
        },
        budget,
    )?;
    Ok(found.get())
}

/// Where Tcl ends the braced word whose `{` stands at `start`: after the `}`
/// at which the raw brace count returns to zero. A backslash hides the next
/// byte. Nothing else is special for Tcl inside the word: a `}` in a
/// comment, in a quoted word or in a bracket closes a level. A grammar that
/// reads those first ends its node elsewhere than Tcl ends the word.
///
/// `None` if the count does not return to zero before `code` ends. A caller
/// that asks only whether the word ends at one place passes the bytes up to
/// that place, and no byte behind it is read.
pub(crate) fn braced_word_end<B: WorkBudget>(
    code: &[u8],
    start: usize,
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    let depth = Cell::new(0_usize);
    let hidden = Cell::new(false);
    let stop = byte_scan::position(
        &code[start..],
        |byte| {
            if hidden.replace(false) {
                return false;
            }
            match byte {
                b'\\' => hidden.set(true),
                b'{' => depth.set(depth.get() + 1),
                b'}' => {
                    depth.set(depth.get().saturating_sub(1));
                    return depth.get() == 0;
                }
                _ => {}
            }
            false
        },
        budget,
    )?;
    Ok(stop.map(|at| start + at + 1))
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

#[cfg(test)]
#[cfg(any(feature = "tcl", feature = "irules"))]
mod readable_slot_tests {
    //! `argument_slots_are_readable` under a lent `FoldCache`, for both
    //! dialects: the answer is the tree's and the scan runs once per
    //! argument list (#1686).

    use super::argument_slot_scans;
    use crate::getter::{BracedWordKinds, Getter};
    use crate::node::{Ancestors, FoldCache, MemoQuestion, Node, Tree};
    use crate::traits::LanguageInfo;

    /// Every `braced_word` of `root`'s tree, each with its ancestor
    /// chain, in source order: what the walks hand the predicate.
    ///
    /// Not `test_support::for_each_node_with_chain`, which refuses a
    /// fixture that does not parse — and an erroring list is the point.
    /// The sort is what gives source order, so the stack's pop order is
    /// free to be whatever `children()` yields.
    fn braced_words(root: Node<'_>, script: u16) -> Vec<(Node<'_>, Vec<Node<'_>>)> {
        let mut out = Vec::new();
        let mut stack = vec![(root, Vec::new())];
        while let Some((node, chain)) = stack.pop() {
            if node.kind_id() == script {
                out.push((node, chain.clone()));
            }
            let mut below = chain;
            below.push(node);
            stack.extend(node.children().map(|child| (child, below.clone())));
        }
        out.sort_by_key(|(node, _)| node.start_byte());
        out
    }

    /// A wide `eval` line with one stray `]` among its arguments is
    /// scanned once under a lent cache, and once per braced word
    /// without one, with every word answering the same either way.
    ///
    /// `eval` evaluates every argument, so each word answers "script"
    /// whether or not its list is readable — this row pins the
    /// *count* only, and `a_cached_answer_is_the_list_s_own` below is
    /// what pins the answer. Deleting the `memo.set` in
    /// `argument_slots_are_readable` fails the first assertion here;
    /// making the memo answer a constant fails only the other test.
    fn scans_once_per_list<L: LanguageInfo + Getter>(label: &str, kinds: &BracedWordKinds) {
        let width = 200;
        let code = crate::test_support::tcl_wide_erroring_eval(width);
        let code = code.as_bytes();
        let tree = Tree::new::<L>(code);
        let root = tree.get_root();
        let words = braced_words(root, kinds.script);
        assert_eq!(
            words.len(),
            width,
            "{label}: the fixture must spell {width} words"
        );

        let cache = FoldCache::default();
        let before = argument_slot_scans::observed();
        let cached: Vec<bool> = words
            .iter()
            .map(|(word, chain)| {
                L::is_braced_literal_slot(
                    word,
                    code,
                    Ancestors::known(chain).with_folds(&cache),
                    kinds,
                )
            })
            .collect();
        assert_eq!(
            argument_slot_scans::observed() - before,
            1,
            "{label}: a walk lending a FoldCache scans the list once"
        );

        let before = argument_slot_scans::observed();
        let uncached: Vec<bool> = words
            .iter()
            .map(|(word, chain)| {
                L::is_braced_literal_slot(word, code, Ancestors::known(chain), kinds)
            })
            .collect();
        assert_eq!(
            argument_slot_scans::observed() - before,
            width,
            "{label}: without a cache every word scans the list"
        );
        // Both vectors are all-`false` by construction (every `eval`
        // argument is a script), so this pins only that the two walks
        // agree on the fixture; the answer dimension is the next test's.
        assert_eq!(
            cached, uncached,
            "{label}: the cached and uncached walks must agree on the fixture"
        );
    }

    /// A well-formed wide `eval` line never scans and never stores: the
    /// `O(1)` `has_error` pre-filter answers before the memo is touched,
    /// which is what keeps every command in a file that parses from
    /// paying for the memo at all. Removing that early return changes no
    /// answer and no error-path count, so only this row sees it — with a
    /// lent cache, where the scan would also leave an entry behind, and
    /// without one.
    fn a_clean_list_is_never_scanned<L: LanguageInfo + Getter>(
        label: &str,
        kinds: &BracedWordKinds,
    ) {
        let width = 200;
        let code = format!("eval {}\n", "{a b} ".repeat(width));
        let code = code.as_bytes();
        let tree = Tree::new::<L>(code);
        let root = tree.get_root();
        assert!(!root.has_error(), "{label}: the fixture must parse cleanly");
        let words = braced_words(root, kinds.script);
        assert_eq!(
            words.len(),
            width,
            "{label}: the fixture must spell {width} words"
        );
        let list = words[0]
            .1
            .last()
            .copied()
            .expect("a braced argument sits under a word_list");

        let cache = FoldCache::default();
        let before = argument_slot_scans::observed();
        for (word, chain) in &words {
            let cached = Ancestors::known(chain).with_folds(&cache);
            let uncached = Ancestors::known(chain);
            assert!(
                !L::is_braced_literal_slot(word, code, cached, kinds)
                    && !L::is_braced_literal_slot(word, code, uncached, kinds),
                "{label}: every `eval` argument is a script"
            );
        }
        assert_eq!(
            argument_slot_scans::observed() - before,
            0,
            "{label}: a list whose owner has no error is never scanned"
        );
        assert!(
            Ancestors::known(&words[0].1)
                .with_folds(&cache)
                .node_memo(MemoQuestion::ArgumentSlotsReadable)
                .expect("the chain lent a cache")
                .get(&list)
                .is_none(),
            "{label}: a list that was never scanned has no stored answer"
        );
    }

    /// Two `after` lines in one tree and under one cache: one whose
    /// list holds the `]` as a slot of its own, and one whose `]` is
    /// inside a script argument. The first's `{100}` keeps the
    /// construct-wide script answer, the second's is the value its
    /// slot names — and asking twice answers the same, so the memo
    /// stores the list's own answer rather than its neighbour's or a
    /// constant.
    fn a_cached_answer_is_the_list_s_own<L: LanguageInfo + Getter>(
        label: &str,
        kinds: &BracedWordKinds,
    ) {
        let code = b"after {100} ] {puts hi}\nafter {100} {puts ]}\n";
        let tree = Tree::new::<L>(code);
        let root = tree.get_root();
        let words = braced_words(root, kinds.script);
        let texts: Vec<&str> = words
            .iter()
            .map(|(word, _)| word.utf8_text(code).expect("ascii fixture"))
            .collect();
        assert_eq!(
            texts,
            ["{100}", "{puts hi}", "{100}", "{puts ]}"],
            "{label}: the fixture must spell a `{{100}}` on each line"
        );

        let cache = FoldCache::default();
        let before = argument_slot_scans::observed();
        let ask = || -> Vec<bool> {
            words
                .iter()
                .map(|(word, chain)| {
                    L::is_braced_literal_slot(
                        word,
                        code,
                        Ancestors::known(chain).with_folds(&cache),
                        kinds,
                    )
                })
                .collect()
        };
        let first = ask();
        let again = ask();
        assert_eq!(
            first,
            [false, false, true, false],
            "{label}: the `{{100}}` after a stray `]` keeps the script answer; \
             the one beside an error *inside* a script is the value its slot names"
        );
        assert_eq!(again, first, "{label}: a second ask answers from the memo");
        assert_eq!(
            argument_slot_scans::observed() - before,
            2,
            "{label}: each erroring list is scanned once across both asks"
        );
    }

    #[test]
    #[cfg(feature = "tcl")]
    fn tcl_scans_an_erroring_list_once_per_walk() {
        scans_once_per_list::<crate::langs::TclCode>(
            "tcl",
            &crate::lang_helpers::tcl::BRACED_WORD_KINDS,
        );
    }

    #[test]
    #[cfg(feature = "tcl")]
    fn tcl_a_cached_answer_is_the_list_s_own() {
        a_cached_answer_is_the_list_s_own::<crate::langs::TclCode>(
            "tcl",
            &crate::lang_helpers::tcl::BRACED_WORD_KINDS,
        );
    }

    #[test]
    #[cfg(feature = "tcl")]
    fn tcl_a_clean_list_is_never_scanned() {
        a_clean_list_is_never_scanned::<crate::langs::TclCode>(
            "tcl",
            &crate::lang_helpers::tcl::BRACED_WORD_KINDS,
        );
    }

    #[test]
    #[cfg(feature = "irules")]
    fn irules_a_clean_list_is_never_scanned() {
        a_clean_list_is_never_scanned::<crate::langs::IrulesCode>(
            "irules",
            &crate::lang_helpers::irules::BRACED_WORD_KINDS,
        );
    }

    #[test]
    #[cfg(feature = "irules")]
    fn irules_scans_an_erroring_list_once_per_walk() {
        scans_once_per_list::<crate::langs::IrulesCode>(
            "irules",
            &crate::lang_helpers::irules::BRACED_WORD_KINDS,
        );
    }

    #[test]
    #[cfg(feature = "irules")]
    fn irules_a_cached_answer_is_the_list_s_own() {
        a_cached_answer_is_the_list_s_own::<crate::langs::IrulesCode>(
            "irules",
            &crate::lang_helpers::irules::BRACED_WORD_KINDS,
        );
    }
}
