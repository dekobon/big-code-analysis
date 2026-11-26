//! What the recorded tree and the source bytes say about one Elixir node.
//! Nothing here writes to the index.

use std::ops::Range;

use super::super::*;

// The node kinds whose children are statements that a `;` or a line end
// separates: the file, the blocks of a call, the body of a `->` clause, a
// parenthesized block and the clauses of an anonymous function.
const SEQUENCES: [&str; 9] = [
    "source",
    "do_block",
    "after_block",
    "rescue_block",
    "catch_block",
    "else_block",
    "body",
    "block",
    "anonymous_function",
];

// The blocks of a `do` block that follow its statements with no separator.
const SECTIONS: [&str; 4] = ["after_block", "rescue_block", "catch_block", "else_block"];

// The quotes of a string, of a charlist and of their heredocs.
const QUOTES: [&str; 4] = ["\"", "'", "\"\"\"", "'''"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Def,
    Defp,
    Defmacro,
    Defmacrop,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Word {
    Quote,
    Module,
    // `defprotocol` and `defimpl`.
    Protocol,
    Definition(Kind),
    // `defguard`, `defguardp` and `defdelegate`.
    Unread,
    Other,
}

impl Word {
    // Whether the word starts a module or a definition, read or not.
    pub(super) fn declares(&self) -> bool {
        !matches!(self, Self::Quote | Self::Other)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Attribute {
    Doc,
    Moduledoc,
    Typedoc,
    // `@callback` and `@macrocallback` take a pending `@doc`.
    Callback,
    Other,
}

// How a child stands to the statement before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Gap {
    // No statement stands before it, or a `;` or a line end lies between.
    Apart,
    // Excluded bytes hide what lies between.
    Hidden,
    // Nothing separates the two: the compiler reads no statement here.
    Joined,
}

pub(super) fn node<'s>(canonical: &CanonicalNodes<'s>, id: NodeId) -> tree_sitter::Node<'s> {
    canonical.nodes[id.0].evidence.node()
}

pub(super) fn children<'c>(canonical: &'c CanonicalNodes<'_>, id: NodeId) -> &'c [NodeId] {
    &canonical.children[canonical.nodes[id.0].children.clone()]
}

pub(super) fn kind(canonical: &CanonicalNodes<'_>, id: NodeId) -> &'static str {
    node(canonical, id).kind()
}

pub(super) fn step<B: WorkBudget>(budget: &B) -> Result<(), SourceError> {
    budget.step().map_err(Into::into)
}

// The children of a node that are no comment, in order. The compiler drops a
// comment, so no count and no position of a child may depend on one. An
// `ERROR` and a missing child stay.
pub(super) fn parts<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    id: NodeId,
    budget: &B,
) -> Result<Vec<NodeId>, B::Error> {
    let mut parts = Vec::new();
    for &child in children(canonical, id) {
        budget.step()?;
        if kind(canonical, child) != "comment" {
            parts.push(child);
        }
    }
    Ok(parts)
}

pub(super) fn covers<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    range: Range<usize>,
    budget: &B,
) -> Result<bool, SourceError> {
    index
        .source
        .covers_with_budget(range, budget)
        .map_err(Into::into)
}

fn spelled(index: &DocumentationIndex<'_>, id: NodeId, word: &[u8]) -> bool {
    let name = node(&index.canonical, id);
    name.kind() == "identifier" && &index.source.bytes()[name.byte_range()] == word
}

// The word a `call` starts with. A first word that is not fully included
// cannot be told from `quote`, so it is read as one.
pub(super) fn word<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    call: NodeId,
    budget: &B,
) -> Result<Word, SourceError> {
    let canonical = &index.canonical;
    let target = match parts(canonical, call, budget).map_err(Into::into)?.first() {
        Some(&target) if kind(canonical, target) == "identifier" => node(canonical, target),
        _ => return Ok(Word::Other),
    };
    if !covers(index, target.byte_range(), budget)? {
        return Ok(Word::Quote);
    }
    Ok(match &index.source.bytes()[target.byte_range()] {
        b"quote" => Word::Quote,
        b"defmodule" => Word::Module,
        b"defprotocol" | b"defimpl" => Word::Protocol,
        b"def" => Word::Definition(Kind::Def),
        b"defp" => Word::Definition(Kind::Defp),
        b"defmacro" => Word::Definition(Kind::Defmacro),
        b"defmacrop" => Word::Definition(Kind::Defmacrop),
        b"defguard" | b"defguardp" | b"defdelegate" => Word::Unread,
        _ => Word::Other,
    })
}

// The body of `defmodule NAME do … end`. The keyword form has none.
pub(super) fn module_body<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    module: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    Ok(match *parts(canonical, module, budget)? {
        [_, _, body] if kind(canonical, body) == "do_block" => Some(body),
        _ => None,
    })
}

pub(super) fn at_operator<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<bool, B::Error> {
    if kind(canonical, statement) != "unary_operator" {
        return Ok(false);
    }
    let operator = parts(canonical, statement, budget)?.first().copied();
    Ok(operator.is_some_and(|operator| kind(canonical, operator) == "@"))
}

// The children of the operand of an `@` statement, and those of its
// argument list when the operand is `NAME ARGUMENTS`. The operand is the last
// child of the statement. Both lists are empty for another operand.
fn operand<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<(Vec<NodeId>, Vec<NodeId>), B::Error> {
    let called = match parts(canonical, statement, budget)?.last() {
        Some(&operand) if kind(canonical, operand) == "call" => parts(canonical, operand, budget)?,
        _ => Vec::new(),
    };
    let arguments = match *called {
        [_, arguments] => parts(canonical, arguments, budget)?,
        _ => Vec::new(),
    };
    Ok((called, arguments))
}

// What a fully included `@` statement sets. `@doc`, `@doc()` and an
// argument list that holds only comments read the attribute and set nothing.
pub(super) fn attribute<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<Attribute, B::Error> {
    let canonical = &index.canonical;
    let (called, arguments) = operand(canonical, statement, budget)?;
    let name = match called.first() {
        Some(&name) if kind(canonical, name) == "identifier" => name,
        _ => return Ok(Attribute::Other),
    };
    if let [open, close] = *arguments
        && kind(canonical, open) == "("
        && kind(canonical, close) == ")"
    {
        return Ok(Attribute::Other);
    }
    Ok(
        match &index.source.bytes()[node(canonical, name).byte_range()] {
            b"doc" => Attribute::Doc,
            b"moduledoc" => Attribute::Moduledoc,
            b"typedoc" => Attribute::Typedoc,
            b"callback" | b"macrocallback" => Attribute::Callback,
            _ => Attribute::Other,
        },
    )
}

// The one value of `NAME VALUE` or `NAME(VALUE)`, from the children of the
// argument list.
fn value(canonical: &CanonicalNodes<'_>, arguments: &[NodeId]) -> Option<NodeId> {
    match *arguments {
        [value] => Some(value),
        [open, value, close] if kind(canonical, open) == "(" && kind(canonical, close) == ")" => {
            Some(value)
        }
        _ => None,
    }
}

// The bytes between the quotes when the value of a documentation attribute
// is one plain string. The grammar gives an escape and an interpolation
// children of their own, and a heredoc other quotes.
pub(super) fn plain<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<Option<Range<usize>>, B::Error> {
    let (_, arguments) = operand(canonical, statement, budget)?;
    let string = value(canonical, &arguments).filter(|&value| kind(canonical, value) == "string");
    let Some(string) = string else {
        return Ok(None);
    };
    let quotes = match *parts(canonical, string, budget)? {
        [open, close] => Some((open, close)),
        [open, content, close] if kind(canonical, content) == "quoted_content" => {
            Some((open, close))
        }
        _ => None,
    };
    Ok(quotes
        .filter(|&(open, close)| kind(canonical, open) == "\"" && kind(canonical, close) == "\"")
        .map(|(open, close)| node(canonical, open).end_byte()..node(canonical, close).start_byte()))
}

// Whether the compiler rejects a character of a string: a line break other
// than LF or CR LF, or a bidirectional formatting character.
pub(in crate::documentation::collector) fn rejected<B: WorkBudget>(
    text: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    for (at, &byte) in text.iter().enumerate() {
        budget.bytes(1)?;
        let next = |offset: usize| text.get(at + offset).copied();
        let found = match byte {
            0x0b | 0x0c => true,
            b'\r' => next(1) != Some(b'\n'),
            0xc2 => next(1) == Some(0x85),
            0xe2 => matches!(
                (next(1), next(2)),
                (Some(0x80), Some(0xa8..=0xae)) | (Some(0x81), Some(0xa6..=0xa9))
            ),
            _ => false,
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

// Whether a line end or a `;` stands in the bytes between two statements.
// `escaped` starts true behind `?\` or `?`, which takes the line feed that
// starts the gap; `lines` is false behind `,`, where no line feed counts. A
// backslash before a line end continues the line, and a comment runs to its
// line end, which no backslash continues.
pub(in crate::documentation::collector) fn separated<B: WorkBudget>(
    gap: &[u8],
    mut escaped: bool,
    lines: bool,
    budget: &B,
) -> Result<bool, B::Error> {
    let mut comment = false;
    for &byte in gap {
        budget.bytes(1)?;
        match byte {
            b'\n' if lines && (comment || !escaped) => return Ok(true),
            b';' if !comment => return Ok(true),
            b'\n' if comment => comment = false,
            b'#' => comment = true,
            b'\\' => escaped = true,
            b'\r' if escaped => {}
            _ => escaped = false,
        }
    }
    Ok(false)
}

// How the child at `start` stands to `before`, the statement or recovery
// before it. The last leaf of `before` decides whether a line feed counts.
fn gap<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    mut before: NodeId,
    start: usize,
    budget: &B,
) -> Result<Gap, SourceError> {
    let end = node(&index.canonical, before).end_byte();
    while let Some(&last) = children(&index.canonical, before).last() {
        step(budget)?;
        before = last;
    }
    let leaf = node(&index.canonical, before);
    if !covers(index, leaf.start_byte()..start, budget)? {
        return Ok(Gap::Hidden);
    }
    let bytes = index.source.bytes();
    let (spelled, gap) = (&bytes[leaf.start_byte()..end], &bytes[end..start]);
    let escaped =
        (leaf.kind() == "char" && spelled == b"?\\") || (leaf.is_error() && spelled == b"?");
    let apart = separated(gap, escaped, spelled != b",", budget).map_err(Into::into)?;
    Ok(if apart { Gap::Apart } else { Gap::Joined })
}

// Whether the grammar recovered at this node itself: an `ERROR`, a missing
// token, or a node with an error that none of its children holds, which is a
// missing token that the grammar shows no node for.
fn recovery<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    id: NodeId,
    budget: &B,
) -> Result<bool, B::Error> {
    let written = node(canonical, id);
    if written.is_error() || written.is_missing() {
        return Ok(true);
    }
    if !written.has_error() {
        return Ok(false);
    }
    for &child in children(canonical, id) {
        budget.step()?;
        if node(canonical, child).has_error() {
            return Ok(false);
        }
    }
    Ok(true)
}

// Whether the grammar recovered in a statement outside the sequences below
// it. A `;` or a line end holds a recovery inside the statement it ends; in
// such a statement no part is known to be what the compiler reads.
pub(super) fn broken<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<bool, B::Error> {
    if !node(canonical, statement).has_error() {
        return Ok(false);
    }
    // Every node of the stack holds an error.
    let mut stack = vec![statement];
    while let Some(id) = stack.pop() {
        budget.step()?;
        let written = node(canonical, id);
        // A sequence holds what its statements and its own tokens lack.
        if SEQUENCES.contains(&written.kind()) {
            continue;
        }
        let known = stack.len();
        for &child in children(canonical, id) {
            budget.step()?;
            if node(canonical, child).has_error() {
                stack.push(child);
            }
        }
        // An `ERROR`, or an error that no child holds: a missing token.
        if written.is_error() || stack.len() == known {
            return Ok(true);
        }
    }
    Ok(false)
}

// Whether a child that is no comment is a statement: a named child of a
// sequence that is no section. An `ERROR` child is named.
fn states(written: &tree_sitter::Node<'_>, sequence: bool) -> bool {
    sequence && written.is_named() && !SECTIONS.contains(&written.kind())
}

// What stands before the next child of a node, comments left out: the last
// statement, a recovery that is the child directly before, and where the first
// torn literal of the file starts.
struct Before {
    statement: Option<NodeId>,
    recovery: Option<NodeId>,
    tear: Option<usize>,
}

impl Before {
    // How a child stands. A child at or behind a torn literal counts as
    // joined: the compiler reads no statement there either. An `ERROR` and a
    // missing node are text that the grammar could not place, and are never
    // joined by the node before them.
    fn gap<B: WorkBudget>(
        &self,
        index: &DocumentationIndex<'_>,
        written: &tree_sitter::Node<'_>,
        states: bool,
        budget: &B,
    ) -> Result<Option<Gap>, SourceError> {
        let start = written.start_byte();
        if self.tear.is_some_and(|tear| start >= tear) {
            return Ok(Some(Gap::Joined));
        }
        let unplaced = written.is_error() || written.is_missing();
        match self.recovery.or(self.statement.filter(|_| states)) {
            Some(before) if !unplaced => gap(index, before, start, budget).map(Some),
            _ => Ok(states.then_some(Gap::Apart)),
        }
    }

    fn pass(&mut self, child: NodeId, states: bool, recovers: bool) {
        self.recovery = recovers.then_some(child);
        if states {
            self.statement = Some(child);
        }
    }
}

// Where the grammar first dropped a quote: the start of the first quote that
// is a child of an `ERROR` node. The compiler pairs the quotes of a file from
// its start, so from there on the two read different bytes as text.
pub(super) fn tear<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    let mut first: Option<usize> = None;
    let root = NodeId(0);
    if !node(canonical, root).has_error() {
        return Ok(first);
    }
    // Every node of the stack holds an error.
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        budget.step()?;
        let torn = node(canonical, id).is_error();
        for &child in children(canonical, id) {
            budget.step()?;
            let written = node(canonical, child);
            if torn && QUOTES.contains(&written.kind()) {
                let at = written.start_byte();
                first = Some(first.map_or(at, |first| first.min(at)));
            }
            if written.has_error() {
                stack.push(child);
            }
        }
    }
    Ok(first)
}

// How each child of a node stands to the statement before it: `None` for a
// child that is no statement and follows no recovery. The result is empty
// where no child can be joined. In a sequence every statement is compared
// with the statement before it. In every node the child after a recovery is
// compared with it: the grammar may have made a part of this node of the
// expression before a statement.
pub(super) fn gaps<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    parent: NodeId,
    tear: Option<usize>,
    budget: &B,
) -> Result<Vec<Option<Gap>>, SourceError> {
    let canonical = &index.canonical;
    let sequence = SEQUENCES.contains(&kind(canonical, parent));
    if !sequence && !node(canonical, parent).has_error() {
        return Ok(Vec::new());
    }
    let mut gaps = Vec::new();
    let mut before = Before {
        statement: None,
        recovery: None,
        tear,
    };
    for &child in children(canonical, parent) {
        step(budget)?;
        let written = node(canonical, child);
        // A comment is no part of the program: it is compared with nothing,
        // and nothing with it.
        if written.kind() == "comment" {
            gaps.push(None);
            continue;
        }
        let states = states(&written, sequence);
        gaps.push(before.gap(index, &written, states, budget)?);
        let recovers = recovery(canonical, child, budget).map_err(Into::into)?;
        before.pass(child, states, recovers);
    }
    Ok(gaps)
}

// Whether two names are the same bytes. Their hash only finds the candidates.
pub(in crate::documentation::collector) fn same<B: WorkBudget>(
    source: &[u8],
    left: Range<usize>,
    right: Range<usize>,
    budget: &B,
) -> Result<bool, B::Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in source[left].iter().zip(&source[right]) {
        budget.bytes(1)?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

// The head of a definition: the first child of its arguments, or what stands
// left of one `when` there.
fn guarded<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    arguments: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    let Some(&first) = parts(canonical, arguments, budget)?.first() else {
        return Ok(None);
    };
    if kind(canonical, first) != "binary_operator" {
        return Ok(Some(first));
    }
    Ok(Some(match *parts(canonical, first, budget)? {
        [left, operator, _] if kind(canonical, operator) == "when" => left,
        _ => first,
    }))
}

// The number of written arguments between `(` and `)`. A spliced argument
// stands for a number of them that only the running code knows.
fn arity<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    list: &[NodeId],
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    let canonical = &index.canonical;
    let arguments = match list {
        [open, arguments @ .., close]
            if kind(canonical, *open) == "(" && kind(canonical, *close) == ")" =>
        {
            arguments
        }
        _ => return Ok(None),
    };
    let mut arity = 0;
    for &argument in arguments {
        budget.step()?;
        if !node(canonical, argument).is_named() {
            continue;
        }
        let target = parts(canonical, argument, budget)?.first().copied();
        if target.is_some_and(|target| spelled(index, target, b"unquote_splicing")) {
            return Ok(None);
        }
        arity += 1;
    }
    Ok(Some(arity))
}

// The name and the source-written arity of `name(…)`. An `unquote` head
// names the definition only when the code runs.
fn named_call<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    head: NodeId,
    budget: &B,
) -> Result<Option<(Range<usize>, usize)>, B::Error> {
    let canonical = &index.canonical;
    let (name, list) = match *parts(canonical, head, budget)? {
        [name, list]
            if kind(canonical, head) == "call"
                && kind(canonical, name) == "identifier"
                && !spelled(index, name, b"unquote") =>
        {
            (name, list)
        }
        _ => return Ok(None),
    };
    let list = parts(canonical, list, budget)?;
    let name = node(canonical, name).byte_range();
    Ok(arity(index, &list, budget)?.map(|arity| (name, arity)))
}

// The name and the source-written arity of a simple head: `f`, `f(…)`, or
// either under one `when`.
fn head<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    arguments: NodeId,
    budget: &B,
) -> Result<Option<(Range<usize>, usize)>, B::Error> {
    let canonical = &index.canonical;
    let Some(head) = guarded(canonical, arguments, budget)? else {
        return Ok(None);
    };
    if kind(canonical, head) == "identifier" {
        return Ok(Some((node(canonical, head).byte_range(), 0)));
    }
    named_call(index, head, budget)
}

// The head of a definition statement: `def HEAD`, `def HEAD, do: …` or
// `def HEAD do … end`, with the head as the first child of the arguments.
pub(super) fn definition<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    statement: NodeId,
    budget: &B,
) -> Result<Option<(Range<usize>, usize)>, B::Error> {
    let canonical = &index.canonical;
    match *parts(canonical, statement, budget)? {
        [_, arguments] | [_, arguments, _] if kind(canonical, arguments) == "arguments" => {
            head(index, arguments, budget)
        }
        _ => Ok(None),
    }
}
