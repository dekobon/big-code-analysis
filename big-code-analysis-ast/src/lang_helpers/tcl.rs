//! Tcl: the leading word of a `command` node, and the grammar's
//! braced-word kind ids.

use crate::Tcl;
use crate::getter::BracedWordKinds;
use crate::node::{Ancestors, Node, WordSlot};

/// The braced-word kinds `Getter::is_subsumed_braced_word`,
/// `Getter::braced_word_op_type` and `Getter::is_braced_script_word` are
/// instantiated with (#1354, #1318): the literal *value* form the guard
/// keys on, the *script* form it gates on holding a command, the comment
/// kind that gate must not mistake for one, the four kinds #1318's role
/// recognition walks — the generic `command`, its `word_list` argument
/// list, the `simple_word` a resolvable command name is spelled with, and
/// the `argument` whose braced child is a parameter default rather than a
/// script — and the `procedure` and `namespace` constructs whose value
/// slots the string and dump classifiers also recognise (#1381).
///
/// It lives here rather than beside the `Getter` impl because three
/// classifiers now read it — `Getter::get_op_type_with_code`,
/// `Checker::is_string_with_code` and `Alterator::keeps_children` — and
/// a second copy is exactly the drift `lang_helpers` exists to prevent
/// (#1381).
///
/// `Namespace`, not `Namespace2`: both are spelled `namespace`, and the
/// suffixed variant is the keyword token inside the construct.
pub(crate) const BRACED_WORD_KINDS: BracedWordKinds = BracedWordKinds {
    value: Tcl::BracedWordSimple as u16,
    script: Tcl::BracedWord as u16,
    comment: Tcl::Comment as u16,
    command: Tcl::Command as u16,
    word_list: Tcl::WordList as u16,
    simple_word: Tcl::SimpleWord as u16,
    argument: Tcl::Argument as u16,
    procedure: Tcl::Procedure as u16,
    namespace: Tcl::Namespace as u16,
    open_brace: Tcl::LBRACE as u16,
};

/// Reads the leading word of a Tcl `command` node when it is a plain
/// `simple_word` (`switch`, `for`, `puts`, …). Returns `None` for any other
/// node kind, for commands whose leading word is computed (`$cmd`, `[cmd]`
/// parse it as `variable_substitution` / `command_substitution`, never
/// statically resolvable to a builtin), and for non-UTF-8 bytes. Shared by
/// the out-of-band control-flow detectors below (grammar-dispatch §10:
/// identity questions read the bytes).
///
/// A *literal* name in a quoted or braced spelling is also unresolved, and
/// that is a deliberate limitation: `"for" {set i 0} {$i < 3} {incr i} {…}`
/// and `{for} …` are legal Tcl that still invoke the builtin, but the
/// grammar parses their name as `quoted_word` / `braced_word` rather than
/// `simple_word`, so they score as plain commands. The `simple_word` gate
/// is what keeps the computed forms out; matching the quoted spellings
/// would mean unquoting the bytes for a style no real Tcl uses.
///
/// Callers dispatch on the returned name so each `command` node resolves it
/// exactly once per metric walk — the helpers below take the resolved
/// identity as a precondition rather than re-deriving it.
///
/// The name is normalised through
/// [`strip_global_qualifier`](crate::lang_helpers::strip_global_qualifier),
/// so `::switch` resolves as `switch` — the same rule
/// `Getter::command_leading_word` applies on the slot-table side. Without
/// it the two disagreed on identical bytes: Halstead and the dump read a
/// qualified `::switch` as a script while cognitive, cyclomatic, ABC and
/// nexits read it as an ordinary call and scored it zero.
#[inline]
#[must_use]
pub fn tcl_command_name<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Tcl::Command as u16 {
        return None;
    }
    command_name_reads::record();
    let name = node.child_by_field_name("name")?;
    if name.kind_id() != Tcl::SimpleWord as u16 {
        return None;
    }
    name.utf8_text(code)
        .map(crate::lang_helpers::strip_global_qualifier)
}

// How many `command` names were read: the walk reads one per `command`
// through the word slot, not one per row or metric that asks (#1697).
// Only the count tells a second read apart. Readable from the root
// crate's tests through `test-support`, since the walk that lends the
// word slot lives there.
crate::observation::counter!(command_name_reads, cfg(any(test, feature = "test-support")));

/// Tcl's `try` has a dedicated kind (unlike `switch`/`for`), but its error
/// handler is a flat run of sibling tokens rather than a wrapper node
/// (issue #1266):
///
/// ```text
/// (try "try" <body> ["on" "error" (arguments) <body>] [(finally)])
/// ```
///
/// The vendored grammar permits at most one `on error` handler and has no
/// `trap` rule at all — Tcl 8.6's multi-handler and `trap` forms degrade to
/// an `ERROR` node, a grammar limitation like the `for` condition in #1264.
/// The scan still tolerates repetition so a grammar bump that adds those
/// forms keeps counting without an edit here.
///
/// Lives here, not in cognitive, since the `Catch` row's `has_handler`
/// gate reads it too (#1697).
///
/// Each handler body is located by structural position, never fixed index
/// (grammar-dispatch §3): it is the child directly following the handler's
/// `arguments` node, found in one pass over the children that remembers
/// whether the previous one was an `arguments` — rather than a state
/// machine over the `on` / `error` / `arguments` run. The
/// grammar's extras are whitespace-only, so nothing can interpose between
/// the `arguments` node and the body it introduces, and `arguments` appears
/// among a `try`'s direct children only in handler position. Repetition
/// costs nothing here: a grammar bump that admits several handlers (or
/// `trap`) is counted without an edit, because each `arguments` yields its
/// own successor. The `finally` clause is a wrapper kind (`Tcl::Finally`)
/// and is never yielded — unconditional cleanup costs nothing, matching the
/// cross-language `finally` convention (#416).
///
/// Shared by Cognitive (which also seeds each body's nesting slot) and
/// Cyclomatic (which counts the bodies); both call it from the arm the
/// `Catch` row's `try` owner reaches.
pub fn tcl_try_handler_bodies<'a>(node: &Node<'a>) -> impl Iterator<Item = Node<'a>> + 'a {
    let mut after_arguments = false;
    node.children().filter(move |child| {
        let is_body = after_arguments;
        after_arguments = child.kind_id() == Tcl::Arguments as u16;
        is_body
    })
}

/// What a `command`'s leading `simple_word` spells, for every word some
/// consumer tells apart (#1697): the raw identity, not a policy reading.
/// `Switch`, `Loop` and `Exit` gate on it, and cyclomatic, cognitive,
/// ABC and nexits branch on it; one `command` is asked by all of them in
/// sequence, and [`tcl_command_word`] answers from the walk's word slot
/// after the first. `Other` is every other leading word — a user proc, a
/// computed callee, a quoted or braced spelling, invalid UTF-8 — cached
/// like any answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum TclCommandWord {
    /// Any leading word that is none of the words below.
    Other = 0,
    /// `switch`.
    Switch,
    /// `for`.
    For,
    /// `return`.
    Return,
    /// `error`.
    Error,
    /// `throw`.
    Throw,
    /// `exit`.
    Exit,
    /// `incr`.
    Incr,
    /// `append`.
    Append,
    /// `lappend`.
    Lappend,
}

impl TclCommandWord {
    const ALL: [Self; 10] = [
        Self::Other,
        Self::Switch,
        Self::For,
        Self::Return,
        Self::Error,
        Self::Throw,
        Self::Exit,
        Self::Incr,
        Self::Append,
        Self::Lappend,
    ];

    /// The word `text` spells (already `::`-normalised), `Other` for
    /// anything else.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        match text {
            "switch" => Self::Switch,
            "for" => Self::For,
            "return" => Self::Return,
            "error" => Self::Error,
            "throw" => Self::Throw,
            "exit" => Self::Exit,
            "incr" => Self::Incr,
            "append" => Self::Append,
            "lappend" => Self::Lappend,
            _ => Self::Other,
        }
    }

    /// The abrupt exits: `return`, `error` (unwinds to the nearest
    /// `catch`), the Tcl 8.6 `throw`, and `exit` (#1270).
    #[inline]
    #[must_use]
    pub fn is_exit(self) -> bool {
        matches!(self, Self::Return | Self::Error | Self::Throw | Self::Exit)
    }

    fn from_slot(value: u8) -> Self {
        Self::ALL
            .get(usize::from(value))
            .copied()
            .unwrap_or(Self::Other)
    }
}

/// [`TclCommandWord`] of `node`: `Other` for anything but a `command`
/// whose leading word resolves (see [`tcl_command_name`]) to one of the
/// words. Read once per `command` per walk when the walk lent a cache —
/// the slot holds the last node asked (#1697).
#[inline]
#[must_use]
pub fn tcl_command_word<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> TclCommandWord {
    if node.kind_id() != Tcl::Command as u16 {
        return TclCommandWord::Other;
    }
    TclCommandWord::from_slot(ancestors.classified(WordSlot::TclCommandWord, node, || {
        tcl_command_name(node, code).map_or(TclCommandWord::Other, TclCommandWord::from_text) as u8
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TclCode;
    use crate::test_support::for_each_node_with_chain;

    // The word slot's budget (#1697): asked about one `command` several
    // times in a row — as cyclomatic, cognitive, ABC and nexits ask on
    // one visit — a walk that lent a cache reads the name once per
    // `command`, and one that did not reads it every time. The fixture
    // has no ask about another node between the repeats, so misses ==
    // commands exactly; the answers agree either way, so only the
    // counter tells the two apart.
    #[cfg(feature = "tcl")]
    #[test]
    fn command_word_is_read_once_per_command_under_a_cache() {
        use crate::node::FoldCache;
        const ASKS: usize = 4;
        let source = b"proc f {a} {\n    switch $a { x { ::return 1 } }\n    for {set i 0} {$i < 3} {incr i} { }\n    puts error\n    $a 1\n}\n";
        let cache = FoldCache::default();
        let mut commands = 0;
        let mut cached = Vec::new();
        let before = command_name_reads::observed();
        for_each_node_with_chain::<TclCode>(source, |node, chain| {
            if node.kind_id() != Tcl::Command as u16 {
                return;
            }
            commands += 1;
            let chain = Ancestors::known(chain).with_folds(&cache);
            let first = tcl_command_word(node, source, chain);
            for _ in 1..ASKS {
                assert_eq!(tcl_command_word(node, source, chain), first);
            }
            cached.push(first);
        });
        let cached_reads = command_name_reads::observed() - before;
        let mut uncached = Vec::new();
        let before = command_name_reads::observed();
        for_each_node_with_chain::<TclCode>(source, |node, chain| {
            if node.kind_id() == Tcl::Command as u16 {
                let chain = Ancestors::known(chain);
                for _ in 0..ASKS {
                    uncached.push(tcl_command_word(node, source, chain));
                }
            }
        });
        let uncached_reads = command_name_reads::observed() - before;
        assert_eq!(
            cached,
            [
                TclCommandWord::Switch,
                TclCommandWord::Other, // the arm list's command: x { ::return 1 }
                TclCommandWord::Return, // ::return, the qualifier stripped
                TclCommandWord::For,
                TclCommandWord::Other, // $i < 3
                TclCommandWord::Incr,
                TclCommandWord::Other, // puts error
                TclCommandWord::Other, // $a 1 — a computed leading word
            ]
        );
        assert_eq!(commands, cached.len());
        assert_eq!(
            uncached,
            cached
                .iter()
                .flat_map(|&w| std::iter::repeat_n(w, ASKS))
                .collect::<Vec<_>>()
        );
        assert_eq!((cached_reads, uncached_reads), (commands, ASKS * commands));
    }

    // A node that is not a `command` is `Other` before any read: the
    // leading word of a `procedure` or a `simple_word` is nobody's
    // command name, and the counter shows no `name` field was read.
    // `tcl_command_name`'s own kind check is what the counter observes
    // — one that recorded before checking would move it here. The outer
    // early return in `tcl_command_word` is unobservable through the
    // counter while that inner check stands (the inner one declines
    // first); it is a slot-write saved, not a correctness guard, and no
    // test can tell its absence apart.
    #[cfg(feature = "tcl")]
    #[test]
    fn command_word_of_a_non_command_is_other_without_a_read() {
        let source = b"proc f {a} { return $a }\n";
        let before = command_name_reads::observed();
        // Collected rather than asserted per node, as the Elixir twin
        // is: a formatted `assert_eq!` message inside the closure leaves
        // its never-taken panic arm uncovered.
        let mut seen = Vec::new();
        for_each_node_with_chain::<TclCode>(source, |node, chain| {
            if node.kind_id() != Tcl::Command as u16 {
                seen.push((
                    node.kind(),
                    tcl_command_name(node, source).is_some(),
                    command_name_reads::observed() - before,
                    tcl_command_word(node, source, Ancestors::known(chain)),
                ));
            }
        });
        assert!(
            seen.len() > 5,
            "the fixture has non-command nodes to ask about"
        );
        let read = seen
            .iter()
            .filter(|&&(_, name, reads, word)| name || reads != 0 || word != TclCommandWord::Other)
            .map(|&(kind, ..)| kind)
            .collect::<Vec<_>>();
        assert_eq!(
            read,
            Vec::<&str>::new(),
            "a non-command was read or classified"
        );
        assert_eq!(command_name_reads::observed(), before);
    }

    // Every discriminant the slot can hold converts back to the variant
    // it came from; a value past the table is `Other`, never a panic.
    #[test]
    fn command_word_round_trips_through_the_slot() {
        for word in TclCommandWord::ALL {
            assert_eq!(TclCommandWord::from_slot(word as u8), word);
        }
        assert_eq!(
            TclCommandWord::from_slot(u8::try_from(TclCommandWord::ALL.len()).expect("fits")),
            TclCommandWord::Other
        );
        for (index, word) in TclCommandWord::ALL.iter().enumerate() {
            assert_eq!(*word as usize, index, "`ALL` is in discriminant order");
        }
    }
}
