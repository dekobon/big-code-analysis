//! Python: the aliased `block` and `lambda` kind sets, and the shape
//! of a docstring.

use crate::Python;
use crate::documentation::budget::{NoBudget, WorkBudget, child, infallible};
use crate::lang_helpers::doc_comment::node_bytes;
use crate::node::{AncestorIter, Ancestors, Cursor, FoldLane, Node};
use std::ops::Range;

/// Whether `node` is a Python `lambda` expression, under either of the
/// grammar's two aliased kind_ids: `Lambda` (196, the concrete
/// production emitted today) and `Lambda2` (197, the currently-unseen
/// hidden alias). `Lambda3` (73) is the `lambda` *keyword* token, not a
/// closure node, and is intentionally excluded.
///
/// This is the single normalization chokepoint for the lambda-alias set
/// — mirroring [`python_is_block`] for the block aliases (#419). It
/// is reused by `PythonCode::is_closure` here, by
/// [`python_lambdas_below_statement_boundary`] and by the cognitive
/// boolean-operator gate in `big-code-analysis`, so a future grammar bump
/// that promotes `Lambda2` to a concrete node is handled in exactly one
/// place rather than drifting across sites (#422). The
/// `python_hidden_block_and_lambda_aliases_stay_unseen` drift guard in
/// `checker.rs` trips on such a bump.
#[inline]
#[must_use]
pub fn python_is_lambda(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Lambda | Python::Lambda2)
}

/// How many `lambda`s enclose `node` below its nearest statement
/// boundary — an `expression_list`, `if`, `for` or `while` — the count
/// `node.count_specific_ancestors::<PythonCode>(ancestors,
/// python_is_lambda, boundary)` takes.
///
/// That climb runs to the root where no boundary is near — a boolean
/// operator in each level of a nested list — and was quadratic in depth
/// asked of every such operator (#1767). Under a walk that lent a
/// `FoldCache` the count is folded down the chain instead, each ancestor
/// once per walk. The fold's state is a `u8`; past 254 lambdas it
/// saturates and the exact climb answers, so the count is exact at every
/// depth and only a lambda nest that deep pays the climb.
#[must_use]
pub fn python_lambdas_below_statement_boundary(
    node: &Node<'_>,
    ancestors: Ancestors<'_, '_>,
) -> usize {
    let climb = || {
        node.count_specific_ancestors::<crate::langs::PythonCode>(
            ancestors,
            python_is_lambda,
            is_statement_boundary,
        )
    };
    if !ancestors.memoises_folds() {
        return climb();
    }
    let folded = ancestors.fold_down(
        node,
        0,
        FoldLane::LambdaDepth,
        0,
        |lambdas, ancestor, _, _| {
            if is_statement_boundary(ancestor) {
                0
            } else if python_is_lambda(ancestor) {
                lambdas.saturating_add(1)
            } else {
                lambdas
            }
        },
    );
    if folded == u8::MAX {
        climb()
    } else {
        usize::from(folded)
    }
}

/// The statement boundary [`python_lambdas_below_statement_boundary`]
/// stops at. Only `expression_list` can change the count: a lambda body
/// is a single expression, so no lambda ever sits *above* an `if` /
/// `for` / `while` statement, and stopping at one is indistinguishable
/// from running to the module root. The three statement kinds are kept
/// as an explicit boundary set rather than a claim about coverage
/// (#1090).
fn is_statement_boundary(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Python::ExpressionList
            | Python::IfStatement
            | Python::ForStatement
            | Python::WhileStatement
    )
}

/// Single normalization point for Python's aliased `block` kind_ids.
///
/// tree-sitter-python lists two `kind_id`s that both stringify to
/// `"block"`: `Block` (135, the hidden `_block` supertype) and
/// `Block2` (160, the concrete production). Empirically only `Block2`
/// is ever emitted for real block bodies (function, class, if/for,
/// while/try/with), so `Block` is dead today — but a future grammar
/// bump could promote the supertype to a concrete node. Routing every
/// "is this a block?" check through here means such a bump is handled
/// at one site instead of silently undercounting at several (issue
/// #419; lesson 2 / 34 / 56 in docs/development/lessons_learned.md).
#[inline]
#[must_use]
pub fn python_is_block(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Block | Python::Block2)
}

/// How many parentheses CPython lets one expression nest in: its
/// tokenizer's `MAXLEVEL` (`Parser/lexer/state.h`). A docstring wrapped
/// deeper than this is a syntax error, so the climb and the descent
/// through `parenthesized_expression` stop here, which also keeps each
/// one constant-time on a file nesting thousands of them. A `u8`, so the
/// memoised climb ([`bare_string_climb`]) saturates against it.
const MAX_PARENTHESES: u8 = 200;

/// Whether `node`, past any parentheses around it, is the sole child of
/// an `expression_statement`: the discarded-value shape a docstring
/// takes. CPython drops the parentheses, so `("""doc""")` is a docstring
/// (#1610). `"a", "b"` has two children, so it is a discarded tuple, not
/// documentation.
#[inline]
#[must_use]
pub fn python_is_bare_statement_value(node: &Node, ancestors: Ancestors<'_, '_>) -> bool {
    enclosing_statement(node, ancestors)
        .is_some_and(|((statement, _), _)| is_bare_statement(&statement))
}

/// Whether `node` is a `parenthesized_expression` around a bare string
/// statement's string, or one of its `(` and `)`. The parentheses belong
/// to the docstring they wrap (#1610): neither code rows nor operators.
///
/// Decided from the statement down, as the string's own climb is, so a
/// string nested past CPython's 200-parenthesis cap keeps every one of
/// its parentheses as code rather than only the outermost. Every
/// parenthesis of a chain asks, and a climb and a descent per parenthesis
/// made a statement `d` deep cost `O(d²)`: a walk that lent a `FoldCache`
/// reads the answer off a memoised fold of the ancestry instead, which
/// unwraps each statement's value once. Any other caller climbs, which
/// the cap bounds where a fold without its memo would run from the root.
#[must_use]
pub fn python_is_bare_string_parenthesis(node: &Node, ancestors: Ancestors<'_, '_>) -> bool {
    let token = matches!(node.kind_id().into(), Python::LPAREN | Python::RPAREN);
    let Some((paren, paren_ancestors)) = (if token {
        ancestors.iter(node).next()
    } else {
        Some((*node, ancestors))
    }) else {
        return false;
    };
    if paren.kind_id() != Python::ParenthesizedExpression {
        return false;
    }
    if ancestors.memoises_folds() {
        // A token's fold stops at its parenthesis, whose state it reads.
        let climb = ancestors.fold_down(
            node,
            usize::from(token),
            FoldLane::BareStringClimb,
            0,
            |climb, ancestor, _, _| bare_string_climb(climb, ancestor),
        );
        return (1..=MAX_PARENTHESES + 1).contains(&climb);
    }
    enclosing_statement(&paren, paren_ancestors)
        .is_some_and(|((statement, _), _)| statement_string(&statement).is_some())
}

/// One step of the fold [`python_is_bare_string_parenthesis`] memoises:
/// the climb from a node through the parentheses above it to the
/// statement holding them, or 0 when that is no bare string statement. A
/// parenthesis lengthens the climb, saturating one past the longest climb
/// [`enclosing_statement`] takes; any other ancestor starts one. A climb
/// past the cap ends at a statement whose string lies past it too, so the
/// length decides nothing there, and no recovery the grammar performs
/// gives a parenthesis holding the string a deeper parenthesis beside it
/// (searched, #1610): the saturation point is kept for the fold to answer
/// as the climb does should one appear, and nothing can pin it.
fn bare_string_climb(climb: u8, ancestor: &Node) -> u8 {
    if ancestor.kind_id() != Python::ParenthesizedExpression {
        u8::from(statement_string(ancestor).is_some())
    } else if climb == 0 {
        0
    } else {
        climb.saturating_add(1).min(MAX_PARENTHESES + 2)
    }
}

/// The nearest ancestor of `node` that is not a parenthesis around it,
/// paired with its ancestry, and the iterator continuing above it.
fn enclosing_statement<'a, 'c>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, 'c>,
) -> Option<((Node<'a>, Ancestors<'a, 'c>), AncestorIter<'a, 'c>)> {
    let mut up = ancestors.iter(node);
    let statement = up
        .by_ref()
        .take(usize::from(MAX_PARENTHESES) + 1)
        .find(|(ancestor, _)| ancestor.kind_id() != Python::ParenthesizedExpression)?;
    Some((statement, up))
}

/// `node` with at most [`MAX_PARENTHESES`] parentheses around it removed:
/// the expression inside, skipping the comments a parenthesized
/// expression may hold. One cursor serves every layer.
fn unparenthesized_with_budget<'a, B: WorkBudget>(
    node: Node<'a>,
    budget: &B,
) -> Result<Node<'a>, B::Error> {
    let mut cursor = node.cursor();
    let mut value = node;
    for _ in 0..MAX_PARENTHESES {
        let Some(inner) = unparenthesized_once_with_budget(&value, &mut cursor, budget)? else {
            break;
        };
        value = inner;
    }
    Ok(value)
}

/// The expression one `parenthesized_expression` holds, or `None` when
/// `node` is not one, read with `cursor`: its first named child that is
/// no extra, so an `ERROR` the grammar recovered to is still its value.
fn unparenthesized_once<'a>(node: &Node<'a>, cursor: &mut Cursor<'a>) -> Option<Node<'a>> {
    infallible(unparenthesized_once_with_budget(node, cursor, &NoBudget))
}

/// [`unparenthesized_once`] under a budget: one step for each child read,
/// the returned one included.
fn unparenthesized_once_with_budget<'a, B: WorkBudget>(
    node: &Node<'a>,
    cursor: &mut Cursor<'a>,
    budget: &B,
) -> Result<Option<Node<'a>>, B::Error> {
    if node.kind_id() != Python::ParenthesizedExpression {
        return Ok(None);
    }
    for inner in node.children_with(cursor) {
        budget.step()?;
        if not_extra(&inner) {
            return Ok(Some(inner));
        }
    }
    Ok(None)
}

/// Whether `node` is one of the grammar's two extras, a `#` comment or
/// a `\` line continuation: the nodes tree-sitter-python admits between
/// any two tokens, which spell no part of the construct they sit in.
#[inline]
#[must_use]
pub fn python_is_extra(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Python::Comment | Python::LineContinuation
    )
}

/// Whether `node` is a named child that spells an expression or a
/// statement rather than one of the grammar's two extras. Both extras
/// are *named* nodes, so `is_named` alone takes the first `\` of
/// `( \` / `\` / `"""doc"""` as the parenthesis's value and reads the
/// string as none (#1830); every selector of "the first real child"
/// skips both.
fn not_extra(node: &Node) -> bool {
    node.is_named() && !python_is_extra(node)
}

/// Whether `statement` is a bare string statement: an
/// `expression_statement` whose sole value is a `string` or
/// `concatenated_string`, past any parentheses around it. Asked of a
/// node alone, with no chain — the LOC walk's `\` finder descends to a
/// statement that is a sibling's descendant (#1819).
#[must_use]
pub fn python_is_bare_string_statement(statement: &Node) -> bool {
    statement_string(statement).is_some()
}

/// The string a bare string statement holds, past any parentheses.
fn statement_string<'a>(statement: &Node<'a>) -> Option<Node<'a>> {
    infallible(statement_string_with_budget(statement, &NoBudget))
}

/// [`statement_string`] under a budget, charged by the descent through
/// the parentheses.
fn statement_string_with_budget<'a, B: WorkBudget>(
    statement: &Node<'a>,
    budget: &B,
) -> Result<Option<Node<'a>>, B::Error> {
    if !is_bare_statement(statement) {
        return Ok(None);
    }
    let Some(inner) = statement.child(0) else {
        return Ok(None);
    };
    let value = unparenthesized_with_budget(inner, budget)?;
    Ok(matches!(
        value.kind_id().into(),
        Python::String | Python::ConcatenatedString
    )
    .then_some(value))
}

/// Whether `node` is a bare string statement's `string`: the statement's
/// sole value itself, or one fragment of an implicitly concatenated
/// literal (`"""doc""" "more"`) that is — CPython joins the fragments
/// into one string, and so does the `concatenated_string` node.
///
/// The one definition of "a string whose value is discarded" for both
/// walks that care: the Halstead getter drops its operands (#695) and
/// the LOC walk bills its rows as comment rows (#415). Whether those rows
/// are also *documentation* is narrower and positional — see
/// [`python_documentation_string_run`] (#1597).
#[inline]
#[must_use]
pub fn python_is_bare_string(node: &Node, ancestors: Ancestors<'_, '_>) -> bool {
    let Some((parent, parent_ancestors)) = ancestors.iter(node).next() else {
        return false;
    };
    if matches!(parent.kind_id().into(), Python::ConcatenatedString) {
        return python_is_bare_statement_value(&parent, parent_ancestors);
    }
    python_is_bare_statement_value(node, ancestors)
}

/// Whether `value` — a `string` or `concatenated_string` — is a
/// documentation string by position: a `str` literal that is the sole
/// value of a bare statement in a docstring, additional-docstring or
/// attribute-docstring place, per `documentation_run`, which holds the
/// rules. One fragment of a concatenation answers `false` on its own:
/// its nearest non-parenthesis ancestor is the `concatenated_string`,
/// not a statement, so the scan finds no body to read — the mechanism,
/// not a second test, which is why no `parent` check precedes it. The
/// `Docstring` row's gate (`roles/python.rs`, #1699): one backward scan
/// over the statement's siblings, which the run helper shares by asking
/// this of a run's final member only. A caller asking it of every
/// member of a run pays one scan per member.
#[must_use]
pub(crate) fn python_is_documentation_string(
    value: &Node,
    code: &[u8],
    ancestors: Ancestors<'_, '_>,
) -> bool {
    documentation_run(value, code, ancestors).is_some()
}

// Each backward scan `documentation_run` makes. The scan is one per run
// — the helper asks at the run's final member only —
// and `one_scan_per_run` pins it by count, since no probe
// can tell one scan per run from two (#1699).
crate::observation::counter!(docstring_runs_scanned);

/// The first statement of the documentation-string run that the bare
/// string statement holding `value` belongs to, or `None` when that
/// statement is not documentation — or holds more than one value, a
/// discarded tuple being no docstring. `value` is a `string` or
/// `concatenated_string`, per [`python_is_bare_statement_value`], and the
/// statement documents where Python tooling reads documentation (#1597):
///
/// - **a docstring** (PEP 257): the first statement of a module, class or
///   function body — the string Python stores as `__doc__`, and the one
///   `help()`, `inspect` and pydoc read. Only a `str` literal is stored:
///   an f-string, a template string or a bytes literal is no docstring;
/// - **an additional docstring** (PEP 257): a string statement directly
///   after another docstring, including an attribute or additional docstring;
/// - **an attribute docstring** (PEP 257 and 258, read by Sphinx autodoc):
///   a string statement directly after an assignment to a name at module
///   or class level, or to an attribute of `self` at the top level of a
///   class's `__init__`.
///
/// "Directly after" skips comments, which are not statements. Any other
/// bare string — a triple-quoted string wrapping commented-out code in
/// the middle of a function, a module docstring written after the
/// imports — is still a comment row (#415) but is not documentation, so
/// the maintainability index charges it as it charges a `#` comment.
/// `code` is read for a literal's prefix and for `__init__`'s names.
///
/// One backward scan answers both: the string statements before it,
/// skipping comments, are the run, and the first other statement ends
/// the run and is the one an attribute docstring is documented by.
fn documentation_run<'a>(
    value: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    infallible(docstring_run_with_budget(value, code, ancestors, &NoBudget)).map(|run| run.first)
}

/// The documentation-string run that the bare string statement holding a
/// value belongs to, as the one backward scan finds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DocstringRun<'a> {
    /// The run's first member statement.
    pub(crate) first: Node<'a>,
    /// The statement holding the value the scan started from.
    pub(crate) last: Node<'a>,
    /// The `module`, `class_definition` or `function_definition` whose body
    /// holds the run.
    pub(crate) owner: Node<'a>,
    /// The accepted statement an attribute run follows; `None` for a run
    /// that opens its body.
    pub(crate) after: Option<Node<'a>>,
}

/// The one backward scan behind [`documentation_run`], under a budget:
/// the run of the bare string statement holding `value`, or `None` when
/// that statement is not documentation. It charges one step for each
/// sibling it takes, the one that ends the scan included, and the
/// predicates charge their own loops. With [`NoBudget`] the answer and the
/// scan count are those of the unbudgeted reader.
pub(crate) fn docstring_run_with_budget<'a, B: WorkBudget>(
    value: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<Option<DocstringRun<'a>>, B::Error> {
    // bca: suppress(nexits) -- the unbudgeted reader's exits and one for each charged call.
    // The literal test is the cheap one; the body's owner may read the
    // enclosing `__init__`'s name and parameters.
    if !is_str_literal_with_budget(value, code, budget)? {
        return Ok(None);
    }
    let Some(((statement, statement_ancestors), up)) = enclosing_statement(value, ancestors) else {
        return Ok(None);
    };
    // A string of a discarded tuple (`"a", "b"`) is no docstring: the
    // statement holds two values. Checked here, the one place, so the
    // row and the run helper cannot differ on it.
    if !is_bare_statement(&statement) {
        return Ok(None);
    }
    docstring_runs_scanned::record();
    let Some((owner, attributes)) = body_owner_with_budget(up, code, budget)? else {
        return Ok(None);
    };
    let mut run = DocstringRun {
        first: statement,
        last: statement,
        owner,
        after: None,
    };
    for sibling in statement_ancestors.siblings_before_with_budget(&statement, budget)? {
        budget.step()?;
        if !not_extra(&sibling) {
            continue;
        }
        if !is_string_statement_with_budget(&sibling, code, budget)? {
            let documented = attributes.documented_by_with_budget(&sibling, code, budget)?;
            run.after = Some(sibling);
            return Ok(documented.then_some(run));
        }
        run.first = sibling;
    }
    Ok(Some(run))
}

/// Credits a consecutive run of documentation strings at its last string.
/// Comments between statements do not break the run. Deciding once keeps
/// additional-docstring recognition linear in the number of statements.
pub fn python_documentation_string_run(
    value: &Node,
    code: &[u8],
    ancestors: Ancestors<'_, '_>,
    mut credit: impl FnMut(usize, usize),
) {
    infallible(completed_docstring_run_with_budget(
        value,
        code,
        ancestors,
        &NoBudget,
        |statement, _| credit_documentation_rows(statement, &mut credit),
    ));
}

/// The run that `value` completes, under a budget: `None` unless `value`
/// is the value of a run's final member. `member` is called with each
/// member statement and its value, first to last. One look-ahead decides
/// whether the statement is the final member, and only then does the one
/// backward scan run ([`docstring_run_with_budget`]). The look-ahead, the
/// scan and the walk each charge one step for each sibling they take.
pub(crate) fn completed_docstring_run_with_budget<'a, B: WorkBudget>(
    value: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    budget: &B,
    mut member: impl FnMut(Node<'a>, Node<'a>),
) -> Result<Option<DocstringRun<'a>>, B::Error> {
    // bca: suppress(nargs, nexits) -- the legacy walk's arguments and a budget; a pass can cancel.
    let Some(((statement, outer), _)) = enclosing_statement(value, ancestors) else {
        return Ok(None);
    };
    if !is_bare_statement(&statement) {
        return Ok(None);
    }
    if string_statement_follows(&statement, code, outer, budget)? {
        return Ok(None);
    }
    // The run's final member. `docstring_run_with_budget` is the backward
    // scan the `Docstring` row's gate reads (#1699) and the one this needs
    // for the run's first member, so it runs once, here: the row's verdict
    // is `Some` by construction, the gate being this call.
    let Some(run) = docstring_run_with_budget(value, code, ancestors, budget)? else {
        return Ok(None);
    };
    for item in
        std::iter::once(run.first).chain(outer.siblings_after_with_budget(&run.first, budget)?)
    {
        budget.step()?;
        if item.start_byte() > run.last.start_byte() {
            break;
        }
        if not_extra(&item)
            && let Some(string) = statement_string_with_budget(&item, budget)?
            && is_str_literal_with_budget(&string, code, budget)?
        {
            member(item, string);
        }
    }
    Ok(Some(run))
}

/// Whether the next statement after `statement` is a string statement:
/// the look-ahead that tells a run's final member from an earlier one.
/// It charges one step for each sibling it takes, the one it tests
/// included.
fn string_statement_follows<'a, B: WorkBudget>(
    statement: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<bool, B::Error> {
    for next in ancestors.siblings_after_with_budget(statement, budget)? {
        budget.step()?;
        if not_extra(&next) {
            return is_string_statement_with_budget(&next, code, budget);
        }
    }
    Ok(false)
}

/// Credits the rows of a string statement's value and of the `(` and `)`
/// around it, which are part of the docstring (#1610); a comment inside
/// the parentheses stays a comment.
fn credit_documentation_rows(statement: Node, credit: &mut impl FnMut(usize, usize)) {
    let Some(mut value) = statement.child(0) else {
        return;
    };
    let mut cursor = value.cursor();
    for _ in 0..MAX_PARENTHESES {
        let Some(inner) = unparenthesized_once(&value, &mut cursor) else {
            break;
        };
        for paren in value
            .children_with(&mut cursor)
            .filter(|child| !child.is_named())
        {
            credit(paren.start_row(), paren.end_row());
        }
        value = inner;
    }
    credit(value.start_row(), value.end_row());
}

/// The assignments a body reads an attribute docstring after: PEP 258's
/// rules for the top level of a module, a class or `__init__`, with the
/// targets Sphinx's `VariableCommentPicker` accepts there. Sphinx also
/// reads the suites of an `if`, `with`, loop or `try` at those levels;
/// PEP 257 does not, and neither does this.
enum Attributes<'code> {
    /// A module or class body: an assignment to a name, or to a tuple of
    /// names — not to a subscript or another object's attribute, which
    /// declares nothing there.
    Names,
    /// The top level of a class's `__init__`: an assignment to an
    /// attribute of its first parameter, whose name this holds (`self`),
    /// alone or in a tuple of targets.
    OfSelf(&'code [u8]),
    /// Any other function body.
    Nothing,
}

impl Attributes<'_> {
    /// Whether `statement`, directly before a string statement, makes that
    /// string an attribute docstring.
    fn documented_by(&self, statement: &Node, code: &[u8]) -> bool {
        infallible(self.documented_by_with_budget(statement, code, &NoBudget))
    }

    /// [`Self::documented_by`] under a budget, charged by the walk of the
    /// target and by the comparison of each receiver spelling.
    fn documented_by_with_budget<B: WorkBudget>(
        &self,
        statement: &Node,
        code: &[u8],
        budget: &B,
    ) -> Result<bool, B::Error> {
        // A `type X = …` alias (PEP 695) declares the name it binds, and
        // Sphinx's `VariableCommentPicker` reads it like an assignment
        // (`AssignmentLikeType` holds `ast.TypeAlias`): a docstring after
        // it or a `#:` comment on or above it documents `X`. It never
        // binds an attribute of `self`.
        if statement.kind_id() == Python::TypeAliasStatement {
            return Ok(matches!(self, Self::Names));
        }
        let Some(target) = assignment_target(statement) else {
            return Ok(false);
        };
        match self {
            Self::Names => binds_with_budget(
                &target,
                |leaf| Ok(leaf.kind_id() == Python::Identifier),
                budget,
            ),
            Self::OfSelf(receiver) => binds_with_budget(
                &target,
                |leaf| is_attribute_of(leaf, receiver, code, budget),
                budget,
            ),
            Self::Nothing => Ok(false),
        }
    }
}

/// Whether `leaf` is an attribute of the name `receiver` spells:
/// `self.x` for the receiver `self`. The two spellings are compared under
/// the budget ([`same_spelling`]).
fn is_attribute_of<B: WorkBudget>(
    leaf: &Node,
    receiver: &[u8],
    code: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    if leaf.kind_id() != Python::Attribute {
        return Ok(false);
    }
    match leaf.child_by_field_name("object") {
        Some(object) if object.kind_id() == Python::Identifier => {
            same_spelling(node_bytes(&object, code), receiver, budget)
        }
        _ => Ok(false),
    }
}

/// What a body makes of the assignments in it, from the ancestors above
/// the statement: `None` when the body holds no documentation at all (an
/// `if` or loop body).
fn body_attributes<'code>(
    up: AncestorIter<'_, '_>,
    code: &'code [u8],
) -> Option<Attributes<'code>> {
    infallible(body_owner_with_budget(up, code, &NoBudget)).map(|(_, attributes)| attributes)
}

/// [`body_attributes`] under a budget, with the node that owns the body:
/// the `module`, or the class or function definition a `block` belongs to.
fn body_owner_with_budget<'a, 'code, B: WorkBudget>(
    mut up: AncestorIter<'a, '_>,
    code: &'code [u8],
    budget: &B,
) -> Result<Option<(Node<'a>, Attributes<'code>)>, B::Error> {
    let Some((container, _)) = up.next() else {
        return Ok(None);
    };
    if container.kind_id() == Python::Module {
        return Ok(Some((container, Attributes::Names)));
    }
    let Some((owner, _)) = up.next().filter(|_| python_is_block(&container)) else {
        return Ok(None);
    };
    Ok(owner_attributes_with_budget(&owner, up, code, budget)?
        .map(|attributes| (owner, attributes)))
}

/// What the body of `owner` — a class or function definition — makes of
/// the assignments in it; `None` for any other owner. `up` continues
/// from `owner`'s parent.
fn owner_attributes<'code>(
    owner: &Node,
    up: AncestorIter<'_, '_>,
    code: &'code [u8],
) -> Option<Attributes<'code>> {
    infallible(owner_attributes_with_budget(owner, up, code, &NoBudget))
}

/// [`owner_attributes`] under a budget, charged by the search for the
/// receiver of `__init__`.
fn owner_attributes_with_budget<'code, B: WorkBudget>(
    owner: &Node,
    up: AncestorIter<'_, '_>,
    code: &'code [u8],
    budget: &B,
) -> Result<Option<Attributes<'code>>, B::Error> {
    if owner.kind_id() == Python::ClassDefinition {
        Ok(Some(Attributes::Names))
    } else if owner.kind_id() == Python::FunctionDefinition {
        let receiver = if is_class_init(owner, up, code) {
            first_parameter_name_with_budget(owner, code, budget)?
        } else {
            None
        };
        Ok(Some(
            receiver.map_or(Attributes::Nothing, Attributes::OfSelf),
        ))
    } else {
        Ok(None)
    }
}

/// Whether `function` is a class's `__init__`, the one method PEP 257
/// reads attribute docstrings in: named so, with the class the first
/// ancestor past its decorators and the class body's block. `up`
/// continues from `function`'s parent.
///
/// The search reads three ancestors at most (#1868): a
/// `decorated_definition` holds its decorators and one definition, never
/// a block or another `decorated_definition`, and no block's parent is
/// a block; `the_kinds_between_a_method_and_its_class_nest_two_deep_at_most`
/// reads that off the pinned grammar.
fn is_class_init(function: &Node, up: AncestorIter<'_, '_>, code: &[u8]) -> bool {
    function
        .child_by_field_name("name")
        .is_some_and(|name| node_bytes(&name, code) == b"__init__")
        && up
            .map(|(node, _)| node)
            .find(|node| node.kind_id() != Python::DecoratedDefinition && !python_is_block(node))
            .is_some_and(|class| class.kind_id() == Python::ClassDefinition)
}

/// The name of `function`'s first parameter, typed or not. The extras
/// are named, and formatters put a comment right after the `(` of an
/// exploded signature (`def __init__(  # noqa: PLR0913`), a `\` too
/// (`def __init__( \`), so both are skipped.
///
/// Charged one step for each child read on the way to the name.
fn first_parameter_name_with_budget<'code, B: WorkBudget>(
    function: &Node,
    code: &'code [u8],
    budget: &B,
) -> Result<Option<&'code [u8]>, B::Error> {
    let Some(parameters) = function.child_by_field_name("parameters") else {
        return Ok(None);
    };
    let Some(first) = child(&parameters, not_extra, budget)? else {
        return Ok(None);
    };
    let name = if first.kind_id() == Python::TypedParameter {
        child(&first, Node::is_named, budget)?
    } else {
        Some(first)
    };
    Ok(name
        .filter(|name| name.kind_id() == Python::Identifier)
        .map(|name| node_bytes(&name, code)))
}

/// Whether two spellings are the same bytes, compared under the budget.
/// Spellings of different length differ at no charge. Otherwise each pair
/// of bytes is charged before it is compared, and the first pair that
/// differs ends the comparison. With [`NoBudget`] the answer is that of
/// `==`.
fn same_spelling<B: WorkBudget>(left: &[u8], right: &[u8], budget: &B) -> Result<bool, B::Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        budget.bytes(1)?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `value` is a literal Python stores as a docstring: a `str`, not
/// an f-string, a template string or bytes — a prefix holding `f`, `t` or
/// `b` in either case makes the statement none, and one fragment of an
/// implicit concatenation is enough.
///
/// Charged one step for each child of a concatenation and one byte for
/// each prefix letter read.
fn is_str_literal_with_budget<B: WorkBudget>(
    value: &Node,
    code: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    if value.kind_id() == Python::ConcatenatedString {
        for fragment in value.children() {
            budget.step()?;
            if fragment.kind_id() == Python::String
                && !has_str_prefix_with_budget(&fragment, code, budget)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    } else {
        has_str_prefix_with_budget(value, code, budget)
    }
}

/// Whether the leading ASCII letters of `literal` hold none of `f`, `t`
/// and `b`, in either case: the prefix of a `str` literal.
fn has_str_prefix_with_budget<B: WorkBudget>(
    literal: &Node,
    code: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    for byte in node_bytes(literal, code) {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        budget.bytes(1)?;
        if matches!(byte.to_ascii_lowercase(), b'f' | b't' | b'b') {
            return Ok(false);
        }
    }
    Ok(true)
}

/// An `expression_statement` holding one value and nothing else: the
/// discarded-value shape a docstring takes.
fn is_bare_statement(statement: &Node) -> bool {
    statement.kind_id() == Python::ExpressionStatement && statement.child_count() == 1
}

/// A bare statement whose value is a `str` literal: a docstring, when it
/// opens its body.
fn is_string_statement_with_budget<B: WorkBudget>(
    statement: &Node,
    code: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    match statement_string_with_budget(statement, budget)? {
        Some(value) => is_str_literal_with_budget(&value, code, budget),
        None => Ok(false),
    }
}

/// Whether an assignment target binds what Sphinx records an attribute
/// docstring for, as its `get_lvar_names` reads a target: `leaf` decides a
/// single one — a name, a subscript or an attribute — and a tuple, list or
/// starred target binds when any element does. Walked with an explicit
/// stack, so a target nested thousands of brackets deep cannot exhaust the
/// thread's stack (#1597).
///
/// Charged one step for the target and one for each child of every node
/// the walk expands; `leaf` charges what it reads itself.
fn binds_with_budget<B: WorkBudget>(
    target: &Node,
    leaf: impl Fn(&Node) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<bool, B::Error> {
    budget.step()?;
    let mut pending = vec![*target];
    while let Some(node) = pending.pop() {
        match node.kind_id().into() {
            Python::Identifier | Python::Subscript | Python::Attribute => {
                if leaf(&node)? {
                    return Ok(true);
                }
            }
            _ => {
                for inner in node.children() {
                    budget.step()?;
                    if inner.is_named() {
                        pending.push(inner);
                    }
                }
            }
        }
    }
    Ok(false)
}

/// Whether `node` owns a body docstring: a function or class definition.
/// The module owns one too and is the file's owner already.
#[must_use]
pub(crate) fn python_is_docstring_owner(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Python::FunctionDefinition | Python::ClassDefinition
    )
}

/// What a `string` node spells between its quotes.
#[derive(Clone, Debug)]
pub(crate) struct StringLiteral {
    /// The bytes between `string_start` and `string_end`.
    pub(crate) interior: Range<usize>,
    /// Whether the prefix holds `r` or `R`, so no escape is decoded.
    pub(crate) raw: bool,
}

/// The interior and the raw flag of `string`, or `None` when the node is
/// no clean `string` between a `string_start` and a `string_end` inside
/// `code`. No parsed source gives the `None` arms on the node's own shape
/// (a `string` with an error, with fewer than two children or with other
/// first and last children); they are kept because a tree and a buffer
/// are separate arguments. Charged one byte for each prefix letter read.
pub(crate) fn python_string_literal<B: WorkBudget>(
    string: &Node,
    code: &[u8],
    budget: &B,
) -> Result<Option<StringLiteral>, B::Error> {
    let children = string.child_count();
    if string.kind_id() != Python::String || string.has_error() || children < 2 {
        return Ok(None);
    }
    let (Some(start), Some(end)) = (string.child(0), string.child(children - 1)) else {
        return Ok(None);
    };
    let interior = start.end_byte()..end.start_byte();
    if start.kind_id() != Python::StringStart
        || end.kind_id() != Python::StringEnd
        || interior.start > interior.end
        || code.get(interior.clone()).is_none()
    {
        return Ok(None);
    }
    let mut raw = false;
    for byte in node_bytes(&start, code) {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        budget.bytes(1)?;
        if byte.eq_ignore_ascii_case(&b'r') {
            raw = true;
            break;
        }
    }
    Ok(Some(StringLiteral { interior, raw }))
}

/// The target of the assignment `statement` holds, annotated or not.
fn assignment_target<'a>(statement: &Node<'a>) -> Option<Node<'a>> {
    (statement.kind_id() == Python::ExpressionStatement)
        .then(|| statement.child(0))
        .flatten()
        .filter(|value| value.kind_id() == Python::Assignment)
        .and_then(|assignment| assignment.child_by_field_name("left"))
}

/// Credits the rows of a Sphinx doc comment that `comment` completes
/// (#1597). Sphinx autodoc reads a comment starting `#:` as an
/// attribute's documentation when it sits on the line of an assignment,
/// after the code, or on the consecutive lines directly above one —
/// every one of them `#:`; a plain `#` line breaks the block. The
/// assignment must be one an attribute docstring may follow
/// (module or class level, or `self.<attr>` at the top of `__init__`).
///
/// A block is decided once, at its last `#:` line, which walks back
/// over the block to credit it: `credit(first_row, last_row)` per line.
/// The grammar places a comment that precedes a body's first statement
/// outside that body, as a child of the class or function, so the
/// statement after it is the body block's first statement.
pub fn python_sphinx_doc_comment<'a>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    mut credit: impl FnMut(usize, usize),
) {
    if !is_sphinx_doc_comment(comment, code) {
        return;
    }
    let documents = match ancestors.siblings_before(comment).next() {
        // The same-line form: `x = 1  #: doc`.
        Some(statement) if statement.end_row() >= comment.start_row() => {
            body_attributes(ancestors.iter(comment), code)
                .is_some_and(|attributes| attributes.documented_by(&statement, code))
        }
        _ => ends_block_above_assignment(comment, code, ancestors),
    };
    if documents {
        credit_block(comment, code, ancestors, &mut credit);
    }
}

/// Whether `comment` is the last line of a `#:` block that sits directly
/// above an assignment an attribute docstring may follow. A block whose
/// next line is another `#:` is decided there instead.
fn ends_block_above_assignment(comment: &Node, code: &[u8], ancestors: Ancestors<'_, '_>) -> bool {
    let Some(next) = ancestors.siblings_after(comment).next() else {
        return false;
    };
    let below = comment.end_row().saturating_add(1);
    if next.start_row() != below || is_sphinx_doc_comment(&next, code) {
        return false;
    }
    if !python_is_block(&next) {
        return body_attributes(ancestors.iter(comment), code)
            .is_some_and(|attributes| attributes.documented_by(&next, code));
    }
    // The comment precedes a body's first statement, so the grammar made
    // it the class's or function's child, ahead of the body block.
    let mut up = ancestors.iter(comment);
    let Some((owner, _)) = up.next() else {
        return false;
    };
    next.children()
        .find(Node::is_named)
        .filter(|first| first.start_row() == below)
        .is_some_and(|first| {
            owner_attributes(&owner, up, code)
                .is_some_and(|attributes| attributes.documented_by(&first, code))
        })
}

/// Credits `comment` and the `#:` lines directly above it, each the
/// first token of its row: one backward pass over one sibling iterator.
fn credit_block(
    comment: &Node,
    code: &[u8],
    ancestors: Ancestors<'_, '_>,
    credit: &mut impl FnMut(usize, usize),
) {
    credit(comment.start_row(), comment.end_row());
    let mut member = *comment;
    let mut before = ancestors.siblings_before(comment).peekable();
    while let Some(above) = before.next() {
        let leads_its_row = before
            .peek()
            .is_none_or(|prev| prev.end_row() < above.start_row());
        if !(is_sphinx_doc_comment(&above, code)
            && above.end_row().saturating_add(1) == member.start_row()
            && leads_its_row)
        {
            break;
        }
        credit(above.start_row(), above.end_row());
        member = above;
    }
}

/// A comment whose text opens with Sphinx's doc-comment marker, `#:`.
fn is_sphinx_doc_comment(node: &Node, code: &[u8]) -> bool {
    node.kind_id() == Python::Comment && node_bytes(node, code).starts_with(b"#:")
}

/// The destructuring wrappers an assignment target nests its elements
/// in: `pattern_list` / `expression_list`, and `tuple_pattern` /
/// `list_pattern`, which each carry two aliased kind ids — the hidden
/// supertype (`TuplePattern` / `ListPattern`, never emitted) and the
/// live node the grammar produces for `(a, b) = …` / `[a, b] = …`
/// (`TuplePattern2` / `ListPattern2`). Matching only the supertype alias
/// silently dropped every parenthesised / bracketed unpacking target,
/// so both are listed per the hidden-alias discipline (#419).
pub const PYTHON_TARGET_PATTERN_KINDS: &[u16] = &[
    Python::PatternList as u16,
    Python::ExpressionList as u16,
    Python::TuplePattern as u16,
    Python::TuplePattern2 as u16,
    Python::ListPattern as u16,
    Python::ListPattern2 as u16,
];

/// Conventional receiver names that denote the enclosing object inside
/// a method body: `self` for instance methods, `cls` for classmethods.
/// Matched against the receiver bytes rather than by resolving the
/// enclosing function's first parameter (#412): a non-conventionally
/// named first parameter is rare enough that under-counting it is
/// preferable to the over-count of treating every foreign-object write
/// as an attribute, which the structural-only proxy did.
const PYTHON_SELF_RECEIVERS: [&[u8]; 2] = [b"self", b"cls"];

/// The attribute name of a `self.<attr>` / `cls.<attr>` `attribute`
/// node, or `None` when the receiver is not a self / cls alias — the
/// bytes the `Member` row's gate and the attribute count both read.
///
/// The node's first child is the receiver:
///   self.x       → receiver is Identifier "self"      → counts
///   db.x         → receiver is Identifier "db"        → foreign, skip
///   self.f.g     → receiver is itself an Attribute    → skip
/// The last named child is the attribute identifier. Only a direct
/// `self.<name>` names an attribute of the class; `self.f.g` writes `g`
/// on `self.f`, so the nested-attribute receiver is rejected. Borrows
/// from `code` so the slice is the canonical dedup key.
#[must_use]
pub fn python_self_attribute_name<'a>(attr: &Node<'a>, code: &'a [u8]) -> Option<&'a [u8]> {
    let receiver = python_self_receiver(attr, code)?;
    // The trailing identifier is the attribute name; the degenerate
    // single-identifier case has none distinct from the receiver.
    let id = attr
        .children()
        .filter(|c| c.kind_id() == Python::Identifier)
        .last()
        .filter(|id| id.start_byte() != receiver.start_byte())?;
    code.get(id.start_byte()..id.end_byte())
}

/// The receiver of `attr` when it is a `self` / `cls` identifier: the
/// first child, read by bytes. The `Member` row's gate asks only this —
/// the attribute's name is the consumer's dedup key, read once there —
/// so a row read costs no child scan.
fn python_self_receiver<'a>(attr: &Node<'a>, code: &[u8]) -> Option<Node<'a>> {
    let receiver = attr
        .child(0)
        .filter(|receiver| receiver.kind_id() == Python::Identifier)?;
    let receiver_bytes = code.get(receiver.start_byte()..receiver.end_byte())?;
    PYTHON_SELF_RECEIVERS
        .contains(&receiver_bytes)
        .then_some(receiver)
}

/// Whether an `attribute` node is a `self.x` / `cls.x` being *bound* —
/// a target of the enclosing `assignment` (`self.x = …`, `self.a,
/// self.b = …`, `self.a, (self.b, c) = …`), as opposed to a read or an
/// operand. The target is the assignment's first child; a destructuring
/// target wraps its elements in [`PYTHON_TARGET_PATTERN_KINDS`], climbed
/// here until the assignment is met. `ancestors` is the node's chain.
///
/// The climb is `O(depth)` in the pattern wrappers above the node, so
/// asking it of every element of a nested target is quadratic in the
/// nesting. No production walk asks it: the attribute count walks down
/// from the assignment's target through those wrappers only, so the
/// climb would answer yes for every element it reaches, and it reads
/// the row's kind table and [`python_self_attribute_name`] instead
/// (#1867). The row tests and the derived suite ask it with real chains.
#[must_use]
pub(crate) fn python_is_bound_self_attribute<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    if python_self_receiver(node, code).is_none() {
        return false;
    }
    let mut below = *node;
    for (ancestor, _) in ancestors.iter(node) {
        let kind = ancestor.kind_id();
        if kind == Python::Assignment as u16 {
            return ancestor
                .child(0)
                .is_some_and(|first| first.id() == below.id());
        }
        if !PYTHON_TARGET_PATTERN_KINDS.contains(&kind) {
            return false;
        }
        below = ancestor;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `python_lambdas_below_statement_boundary` answers the climb's
    /// count for every node, folded through a walk's cache or climbed
    /// without one (#1767).
    ///
    /// The fixture puts a boolean operator under each of 300 nested
    /// lambdas, past the fold's `u8` saturation at 255, so the deepest
    /// operators can only answer right through the exact fallback; a
    /// parenthesised `yield` (an `expression_list`) resets the count
    /// mid-nest, and an `if` and a `for` sit above it all.
    #[cfg(feature = "python")]
    #[test]
    fn lambdas_below_statement_boundary_match_the_climb_past_saturation() {
        use crate::langs::PythonCode;
        use crate::node::FoldCache;
        use crate::test_support::for_each_node_with_chain;
        let depth = 300;
        let source = format!(
            "def g():\n    if c:\n        for i in r:\n            x = {}a and b{}\n            y = lambda: (yield (lambda: a or b), 1)\n",
            "lambda: (a and ".repeat(depth),
            ")".repeat(depth)
        );
        let cache = FoldCache::default();
        let (mut deepest, mut reset) = (0_usize, false);
        for_each_node_with_chain::<PythonCode>(source.as_bytes(), |node, chain| {
            if node.kind_id() != Python::BooleanOperator {
                return;
            }
            let climbed = node.count_specific_ancestors::<PythonCode>(
                Ancestors::known(chain),
                python_is_lambda,
                is_statement_boundary,
            );
            let folded = python_lambdas_below_statement_boundary(
                node,
                Ancestors::known(chain).with_folds(&cache),
            );
            let chained = python_lambdas_below_statement_boundary(node, Ancestors::known(chain));
            assert_eq!(
                (folded, chained),
                (climbed, climbed),
                "(folded, chained) must both be the climb's count"
            );
            deepest = deepest.max(climbed);
            // `a or b` under the `yield`'s `expression_list`: one lambda
            // below the boundary, though two enclose it.
            reset |= node.start_row() == 4 && climbed == 1;
        });
        assert_eq!(deepest, depth, "the fixture must reach past saturation");
        assert!(reset, "the `expression_list` must reset the count");
    }

    /// Every `parenthesized_expression`, `(` and `)` of `source`, each
    /// asked through a memoised walk and through its chain alone, which
    /// takes the climb. A chainless caller climbs the same way, by
    /// `Node::parent`, at a cost that would make this test quadratic.
    #[cfg(feature = "python")]
    fn bare_string_parenthesis_answers(source: &str) -> Vec<[bool; 2]> {
        use crate::langs::PythonCode;
        use crate::node::{FoldCache, Tree};
        let tree = Tree::new::<PythonCode>(source.as_bytes());
        let cache = FoldCache::default();
        let mut answers = Vec::new();
        // Pre-order with the chain the metric walk keeps, so the fold's
        // per-depth slots are reused as they are there.
        let mut stack = vec![(tree.get_root(), 0_usize)];
        let mut chain: Vec<Node> = Vec::new();
        while let Some((node, depth)) = stack.pop() {
            chain.truncate(depth);
            if matches!(
                node.kind_id().into(),
                Python::ParenthesizedExpression | Python::LPAREN | Python::RPAREN
            ) {
                answers.push([
                    python_is_bare_string_parenthesis(
                        &node,
                        Ancestors::known(&chain).with_folds(&cache),
                    ),
                    python_is_bare_string_parenthesis(&node, Ancestors::known(&chain)),
                ]);
            }
            chain.push(node);
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
        answers
    }

    /// The memoised fold answers as the climb it replaced does (#1610),
    /// on both sides of CPython's cap: a bare string under 200
    /// parentheses keeps all of them as its own, one under 201 none — a
    /// discarded f-string is a bare string too, if no docstring — and a
    /// parenthesized value, a tuple and an assignment none.
    /// The climb's answers are the oracle, so a fold that lost its reset
    /// (`climb == 0`) fails here before any metric. The saturation point
    /// and the window's upper edge are not observable: on every tree the
    /// grammar produces, a parenthesis chain either ends at its string
    /// within the cap or climbs past it, and both sides agree on each. A
    /// perturbation of either edge fails nothing (measured), so the
    /// arithmetic in [`bare_string_climb`] is argued in its doc, not
    /// pinned here.
    #[cfg(feature = "python")]
    #[test]
    fn memoised_climb_answers_as_the_climb_does() {
        let nest = |depth: usize, inner: &str| {
            format!("{}{inner}{}", "(".repeat(depth), ")".repeat(depth))
        };
        let cases = [
            (nest(199, "\"s\""), 199 * 3),
            (nest(200, "\"s\""), 200 * 3),
            (nest(201, "\"s\""), 0),
            (
                format!("def f():\n    {}\n", nest(3, "\"\"\"d\"\"\"")),
                3 * 3,
            ),
            ("(  # c\n    \"s\"\n)\n".to_owned(), 3),
            ("(\"a\" \"b\")\n".to_owned(), 3),
            (nest(3, "x"), 0),
            ("(\"a\", \"b\")\n".to_owned(), 0),
            (nest(3, "f\"x\""), 3 * 3),
            (format!("x = {}\n", nest(1_000, "\"s\"")), 0),
            (format!("{}\n(\"s\"\n", nest(2, "\"s\"")), 2 * 3),
            // A `\` run and a comment before the string: both extras are
            // named, and the value is the string past them (#1830).
            // Measured 0 before the fix, every one of the four.
            ("( \\\n\\\n# c\n\"\"\"doc\"\"\" )\n".to_owned(), 3),
            ("( \\\n\\\n# c\n(\"\"\"doc\"\"\") )\n".to_owned(), 2 * 3),
            ("( \\\n\\\n(\"\"\"doc\"\"\")\n)\n".to_owned(), 2 * 3),
            ("( \\\n\\\n# c\n\"\"\"doc\"\"\" \\\n\\\n)\n".to_owned(), 3),
            // The same prefix before a name and before a tuple: no bare
            // string, so the extras are skipped to a value that is none.
            ("( \\\n\\\n# c\nx )\n".to_owned(), 0),
            ("( \\\n\\\n# c\n\"a\", \"b\" )\n".to_owned(), 0),
        ];
        for (source, documenting) in cases {
            let answers = bare_string_parenthesis_answers(&source);
            assert!(!answers.is_empty(), "{source:.40}");
            for [memoised, climbed] in &answers {
                assert_eq!(memoised, climbed, "{source:.40}");
            }
            let found = answers.iter().filter(|[memoised, ..]| *memoised).count();
            assert_eq!(found, documenting, "{source:.40}");
        }
    }

    /// `is_class_init` finds a method's class within three ancestors
    /// because the kinds it skips do not nest: the one pair the grammar
    /// allows is a `decorated_definition` in a `block` (#1868). A
    /// grammar bump that puts a block directly in a block, or a
    /// decorated definition in another, makes that climb as long as the
    /// nest and must change this with it.
    // test-lang-gates: hand-written(python) — reads the grammar crate's
    //     `NODE_TYPES`, which exists only under the `python` feature
    #[cfg(feature = "python")]
    #[test]
    fn the_kinds_between_a_method_and_its_class_nest_two_deep_at_most() {
        let nesting = crate::test_support::nesting_among(tree_sitter_python::NODE_TYPES, |kind| {
            matches!(kind, "decorated_definition" | "block")
        });
        assert_eq!(
            nesting,
            [(
                "block".to_owned(),
                ["decorated_definition".to_owned()].into()
            )]
            .into()
        );
    }

    /// The budgeted docstring reader: what it answers, what it reports and what
    /// it charges. One gate for the module, as every test here parses Python.
    #[cfg(feature = "python")]
    mod reader {
        use super::*;
        use crate::documentation::SourceError;
        use crate::langs::PythonCode;
        use crate::test_support::for_each_node_with_chain;
        use std::cell::Cell;

        /// Counts `step` calls and sums `bytes` charges, and can fail on one of either.
        #[derive(Default)]
        struct Counting {
            steps: Cell<usize>,
            bytes: Cell<usize>,
            fail_at_step: Option<usize>,
            fail_at_byte: Option<usize>,
        }

        impl Counting {
            fn failing_at_step(step: usize) -> Self {
                Self {
                    fail_at_step: Some(step),
                    ..Self::default()
                }
            }

            fn failing_at_byte(byte: usize) -> Self {
                Self {
                    fail_at_byte: Some(byte),
                    ..Self::default()
                }
            }
        }

        impl WorkBudget for Counting {
            type Error = SourceError;

            fn checkpoint(&self) -> Result<(), SourceError> {
                Ok(())
            }

            fn step(&self) -> Result<(), SourceError> {
                self.steps.set(self.steps.get() + 1);
                if self.fail_at_step == Some(self.steps.get()) {
                    Err(SourceError::Cancelled)
                } else {
                    Ok(())
                }
            }

            fn bytes(&self, count: usize) -> Result<(), SourceError> {
                self.bytes.set(self.bytes.get() + count);
                if self
                    .fail_at_byte
                    .is_some_and(|limit| self.bytes.get() >= limit)
                {
                    Err(SourceError::Cancelled)
                } else {
                    Ok(())
                }
            }
        }

        /// A run as `(first start, last start, owner kind, owner start, after start)`.
        type Run = (usize, usize, &'static str, usize, Option<usize>);

        /// What one completed-run call gave: the run, the members as
        /// `(statement start, value start)`, the steps, the bytes and the scans.
        type Completed = (
            Result<Option<Run>, SourceError>,
            Vec<(usize, usize)>,
            usize,
            usize,
            usize,
        );

        fn run_tuple(run: DocstringRun<'_>) -> Run {
            (
                run.first.start_byte(),
                run.last.start_byte(),
                run.owner.kind(),
                run.owner.start_byte(),
                run.after.map(|after| after.start_byte()),
            )
        }

        /// Asks `ask` of the one node of `kind` that starts at `start`.
        fn at_node<R>(
            source: &[u8],
            kind: &str,
            start: usize,
            mut ask: impl FnMut(&Node<'_>, Ancestors<'_, '_>) -> R,
        ) -> R {
            let mut answers = Vec::new();
            for_each_node_with_chain::<PythonCode>(source, |node, chain| {
                if node.kind() == kind && node.start_byte() == start {
                    answers.push(ask(node, Ancestors::known(chain)));
                }
            });
            assert_eq!(answers.len(), 1, "one {kind} at {start}");
            answers.remove(0)
        }

        fn completed_with(
            value: &Node<'_>,
            code: &[u8],
            ancestors: Ancestors<'_, '_>,
            budget: &Counting,
        ) -> Completed {
            let mut members = Vec::new();
            let scans = docstring_runs_scanned::observed();
            let result = completed_docstring_run_with_budget(
                value,
                code,
                ancestors,
                budget,
                |statement, value| {
                    members.push((statement.start_byte(), value.start_byte()));
                },
            )
            .map(|run| run.map(run_tuple));
            (
                result,
                members,
                budget.steps.get(),
                budget.bytes.get(),
                docstring_runs_scanned::observed() - scans,
            )
        }

        /// The completed-run call at the one node of `kind` at `start`, counted.
        fn completed(source: &[u8], kind: &str, start: usize) -> Completed {
            at_node(source, kind, start, |value, ancestors| {
                completed_with(value, source, ancestors, &Counting::default())
            })
        }

        /// The same call cancelled at each step from 1 to `steps`, and then one step
        /// later, where it must give `expected`.
        fn completed_fails_at_each_step(
            source: &[u8],
            kind: &str,
            start: usize,
            steps: usize,
            expected: Option<Run>,
        ) {
            at_node(source, kind, start, |value, ancestors| {
                for step in 1..=steps {
                    let (result, ..) =
                        completed_with(value, source, ancestors, &Counting::failing_at_step(step));
                    assert_eq!(result, Err(SourceError::Cancelled), "step {step}");
                }
                let (result, _, counted, ..) = completed_with(
                    value,
                    source,
                    ancestors,
                    &Counting::failing_at_step(steps + 1),
                );
                assert_eq!(result, Ok(expected));
                assert_eq!(counted, steps);
            });
        }

        #[test]
        fn docstring_run_record_names_owner_members_and_assignment() {
            let source = concat!(
                "\"\"\"a\"\"\"\n\"\"\"b\"\"\"\n# c\n\"\"\"c\"\"\"\nx = 1\n\"\"\"d\"\"\"\n",
                "class C:\n    \"\"\"k\"\"\"\n    def m(self):\n        \"\"\"m\"\"\"\n"
            )
            .as_bytes();
            assert_eq!(source.len(), 96);
            let mut runs = Vec::new();
            for_each_node_with_chain::<PythonCode>(source, |node, chain| {
                if node.kind() == "string" {
                    let run = infallible(docstring_run_with_budget(
                        node,
                        source,
                        Ancestors::known(chain),
                        &NoBudget,
                    ));
                    runs.push((node.start_byte(), run.map(run_tuple)));
                }
            });
            assert_eq!(
                runs,
                [
                    (0, Some((0, 0, "module", 0, None))),
                    (8, Some((0, 8, "module", 0, None))),
                    (20, Some((0, 20, "module", 0, None))),
                    (34, Some((34, 34, "module", 0, Some(28)))),
                    (55, Some((55, 55, "class_definition", 42, None))),
                    (88, Some((88, 88, "function_definition", 67, None))),
                ]
            );
        }

        #[test]
        fn completed_docstring_run_polls_once_per_sibling_and_reports_members() {
            let source: &[u8] =
                b"\"\"\"a\"\"\"\n\"\"\"b\"\"\"\n# c\n\"\"\"c\"\"\"\nx = 1\n\"\"\"d\"\"\"\n";
            // expected: the look-ahead takes `b`, a string statement.
            assert_eq!(completed(source, "string", 0), (Ok(None), vec![], 1, 0, 0));
            // expected: the look-ahead takes `# c` and `c`.
            assert_eq!(completed(source, "string", 8), (Ok(None), vec![], 2, 0, 0));
            // expected: 9 = 1 (look-ahead: `x = 1`) + 3 (scan: `# c`, `b`, `a`)
            //   + 5 (walk: `a`, `b`, `# c`, `c` and the `x = 1` that ends it).
            let run = Some((0, 20, "module", 0, None));
            assert_eq!(
                completed(source, "string", 20),
                (Ok(run), vec![(0, 0), (8, 8), (20, 20)], 9, 0, 1)
            );
            completed_fails_at_each_step(source, "string", 20, 9, run);
            // expected: 3 = 1 (scan: `x = 1`) + 1 (the assignment target) + 1 (walk: `d`).
            assert_eq!(
                completed(source, "string", 34),
                (
                    Ok(Some((34, 34, "module", 0, Some(28)))),
                    vec![(34, 34)],
                    3,
                    0,
                    1
                )
            );
        }

        #[test]
        fn docstring_run_skips_unnamed_siblings() {
            // A `;` between two statements is an unnamed sibling: each pass takes it at
            // the cost of one step and goes on.
            let source: &[u8] = b"\"a\";\n\"b\"\nx = 1\n";
            assert_eq!(completed(source, "string", 0), (Ok(None), vec![], 2, 0, 0));
            // expected: 7 = 1 (look-ahead: `x = 1`) + 2 (scan: `;`, `a`)
            //   + 4 (walk: `a`, `;`, `b` and the `x = 1` that ends it).
            assert_eq!(
                completed(source, "string", 5),
                (
                    Ok(Some((0, 5, "module", 0, None))),
                    vec![(0, 0), (5, 5)],
                    7,
                    0,
                    1
                )
            );
            // expected: 4 = 2 (scan: `;`, `x = 1`) + 1 (the assignment target) + 1 (walk).
            for source in [&b"x = 1;\n\"x doc\"\n"[..], b"x = 1; \"attribute\"\n"] {
                assert_eq!(
                    completed(source, "string", 7),
                    (
                        Ok(Some((7, 7, "module", 0, Some(0)))),
                        vec![(7, 7)],
                        4,
                        0,
                        1
                    )
                );
            }
            let source: &[u8] = b"def f():\n    \"a\"; \"b\"\n    return 1\n";
            assert_eq!(completed(source, "string", 13), (Ok(None), vec![], 2, 0, 0));
            // expected: 7 = 1 (look-ahead: `return 1`) + 2 (scan) + 4 (walk).
            assert_eq!(
                completed(source, "string", 18),
                (
                    Ok(Some((13, 18, "function_definition", 0, None))),
                    vec![(13, 13), (18, 18)],
                    7,
                    0,
                    1
                )
            );
            let source: &[u8] = b"\"\"\"a\nA\"\"\"; \"\"\"b\nB\"\"\"\n";
            assert_eq!(completed(source, "string", 0), (Ok(None), vec![], 2, 0, 0));
            // expected: 5 = 0 (look-ahead: no sibling) + 2 (scan: `;`, `a`)
            //   + 3 (walk: `a`, `;`, `b`).
            assert_eq!(
                completed(source, "string", 11),
                (
                    Ok(Some((0, 11, "module", 0, None))),
                    vec![(0, 0), (11, 11)],
                    5,
                    0,
                    1
                )
            );
        }

        #[test]
        fn string_literal_test_polls_per_fragment_and_prefix_letter() {
            for fragments in [3, 1_500] {
                let source = format!("{}\n", vec!["\"a\""; fragments].join(" "));
                let source = source.as_bytes();
                // expected: 2n + 1 = n (the reader's prefix test: one for each fragment)
                //   + 1 (walk: the statement) + n (the walk's prefix test).
                let steps = 2 * fragments + 1;
                let run = Some((0, 0, "module", 0, None));
                assert_eq!(
                    completed(source, "concatenated_string", 0),
                    (Ok(run), vec![(0, 0)], steps, 0, 1)
                );
                completed_fails_at_each_step(source, "concatenated_string", 0, steps, run);
                // A fragment is not the value of its statement.
                for fragment in 0..fragments {
                    assert_eq!(
                        completed(source, "string", 4 * fragment),
                        (Ok(None), vec![], 0, 0, 0)
                    );
                }
            }
            // expected: 2 steps (fragments `"a"` and `Rb"b"`) and 2 bytes (`R`, `b`).
            assert_eq!(
                completed(b"\"a\" Rb\"b\" \"c\"\n", "concatenated_string", 0),
                (Ok(None), vec![], 2, 2, 0)
            );
            // expected: 3 steps (look-ahead: `y = 1`; walk: the statement and the
            //   `y = 1` that ends it) and 16 bytes (8 prefix letters, read by the reader
            //   and by the walk).
            assert_eq!(
                completed(b"rrrruuuu\"x\"\ny = 1\n", "string", 0),
                (Ok(Some((0, 0, "module", 0, None))), vec![(0, 0)], 3, 16, 1)
            );
        }

        #[test]
        fn rejected_assignment_target_is_traversed_under_the_budget() {
            for (targets, string) in [(3, 21), (1_500, 12_393)] {
                let names: Vec<_> = (0..targets).map(|at| format!("a.x{at}")).collect();
                let source = format!("{} = v\n\"no\"\n", names.join(", "));
                let source = source.as_bytes();
                // expected: 2n + 1 = 1 (scan: the assignment) + 1 (the target)
                //   + (2n - 1) (the children of the `pattern_list`: n attributes and
                //   n - 1 commas). No attribute is a name, so the run is rejected.
                let steps = 2 * targets + 1;
                assert_eq!(
                    completed(source, "string", string),
                    (Ok(None), vec![], steps, 0, 1)
                );
                completed_fails_at_each_step(source, "string", string, steps, None);
            }
        }

        #[test]
        fn parenthesized_value_is_found_past_extras_under_the_budget() {
            for (comments, string) in [(3, 14), (1_500, 6_002)] {
                let source = format!("(\n{}\"doc\"\n)\n", "# c\n".repeat(comments));
                let source = source.as_bytes();
                // expected: n + 3 = 1 (walk: the statement) + (n + 2) (the children of
                //   the parentheses up to the value: `(`, n comments and the string).
                let steps = comments + 3;
                let run = Some((0, 0, "module", 0, None));
                assert_eq!(
                    completed(source, "string", string),
                    (Ok(run), vec![(0, 2 + 4 * comments)], steps, 0, 1)
                );
                assert_eq!(string, 2 + 4 * comments);
                completed_fails_at_each_step(source, "string", string, steps, run);
            }
        }

        #[test]
        fn docstring_run_rejects_a_prefixed_literal_before_the_scan() {
            let read = |source: &[u8], kind: &str, start: usize| {
                at_node(source, kind, start, |value, ancestors| {
                    let budget = Counting::default();
                    let scans = docstring_runs_scanned::observed();
                    let run = docstring_run_with_budget(value, source, ancestors, &budget)
                        .map(|run| run.map(run_tuple));
                    (
                        run,
                        budget.steps.get(),
                        budget.bytes.get(),
                        docstring_runs_scanned::observed() - scans,
                    )
                })
            };
            // expected: one prefix letter is read, and no sibling.
            for source in [
                &b"def f():\n    b\"audit\"\n    pass"[..],
                b"def f():\n    f\"audit\"\n    pass",
            ] {
                assert_eq!(read(source, "string", 13), (Ok(None), 0, 1, 0));
            }
            assert_eq!(
                read(b"x = 1\nf\"audit\"\n", "string", 6),
                (Ok(None), 0, 1, 0)
            );
            // expected: 2 steps (fragments `"a"` and `b"b"`) and 1 byte (`b`).
            assert_eq!(
                read(
                    b"def f():\n    \"a\" b\"b\"\n    pass\n",
                    "concatenated_string",
                    13
                ),
                (Ok(None), 2, 1, 0)
            );
            // The control: `r` is read and is no rejection.
            assert_eq!(
                read(b"def f():\n    r\"audit\"\n    pass", "string", 13),
                (Ok(Some((13, 13, "function_definition", 0, None))), 0, 1, 1)
            );
        }

        #[test]
        fn init_receiver_is_found_past_extras_under_the_budget() {
            for (comments, string, assignment) in [(2, 93, 74), (1_500, 18_069, 18_050)] {
                let source = format!(
                    "class C:\n    def __init__(\n{}        self):\n{}",
                    "        # c\n".repeat(comments),
                    "        self.a = 1\n        \"a doc\"\n"
                );
                let source = source.as_bytes();
                // expected: n + 5 = (n + 2) (the children of `parameters` up to the
                //   receiver: `(`, n comments and `self`) + 1 (scan: the assignment)
                //   + 1 (the target) + 1 (walk: the statement).
                // expected: 4 bytes, the four pairs of `self` and `self`.
                let steps = comments + 5;
                let run = Some((string, string, "function_definition", 13, Some(assignment)));
                assert_eq!(
                    completed(source, "string", string),
                    (Ok(run), vec![(string, string)], steps, 4, 1)
                );
                completed_fails_at_each_step(source, "string", string, steps, run);
            }
            // A typed receiver: its name is the first named child of the parameter.
            // expected: 6 = 2 (`(` and the typed parameter) + 1 (the name inside it)
            //   + 1 (scan) + 1 (the target) + 1 (walk).
            let source: &[u8] =
            b"class C:\n    def __init__(self: \"C\"):\n        self.a = 1\n        \"a doc\"\n";
            let string = source.len() - 8;
            let run = Some((string, string, "function_definition", 13, Some(string - 19)));
            assert_eq!(
                completed(source, "string", string),
                (Ok(run), vec![(string, string)], 6, 4, 1)
            );
            completed_fails_at_each_step(source, "string", string, 6, run);
        }

        #[test]
        fn init_receiver_spelling_is_compared_under_the_budget() {
            let receiver = "s".repeat(20_000);
            // The receiver of `__init__` and the object of the assignment target.
            let source = |object: &str| {
                format!(
                    "class C:\n    def __init__({receiver}):\n        {object}.a = 1\n{}",
                    "        \"a doc\"\n"
                )
            };
            let ask = |object: &str, budget: &Counting| {
                let source = source(object);
                let string = source.len() - 8;
                let assignment = string - 15 - object.len();
                let completed = at_node(source.as_bytes(), "string", string, |value, ancestors| {
                    completed_with(value, source.as_bytes(), ancestors, budget)
                });
                (completed, string, assignment)
            };
            // Equal spellings: every pair is charged.
            // expected: 5 steps = 2 (`(` and the receiver) + 1 (scan: the assignment)
            //   + 1 (the target) + 1 (walk: the statement).
            let (completed, string, assignment) = ask(&receiver, &Counting::default());
            let run = Some((string, string, "function_definition", 13, Some(assignment)));
            assert_eq!(completed, (Ok(run), vec![(string, string)], 5, 20_000, 1));
            // Both sides of the count: the last pair is charged before it is compared,
            // and no charge follows it.
            let (completed, ..) = ask(&receiver, &Counting::failing_at_byte(20_000));
            assert_eq!(completed.0, Err(SourceError::Cancelled));
            let (completed, ..) = ask(&receiver, &Counting::failing_at_byte(20_001));
            assert_eq!((completed.0, completed.3), (Ok(run), 20_000));
            // A late mismatch: pair 19,999 of 20,000 differs and one equal pair follows.
            // expected: 19,999 bytes, the pair that differs included and nothing after it;
            //   4 steps, as the rejected run has no walk.
            let late = format!("{}x{}", "s".repeat(19_998), "s");
            assert_eq!(late.len(), receiver.len());
            let (completed, ..) = ask(&late, &Counting::default());
            assert_eq!(completed, (Ok(None), vec![], 4, 19_999, 1));
            let (completed, ..) = ask(&late, &Counting::failing_at_byte(19_999));
            assert_eq!(completed.0, Err(SourceError::Cancelled));
            let (completed, ..) = ask(&late, &Counting::failing_at_byte(20_000));
            assert_eq!((completed.0, completed.3), (Ok(None), 19_999));
            // Spellings of different length, each a prefix of the other: no pair is
            // compared and nothing is charged.
            for object in [format!("{receiver}s"), "s".repeat(19_999)] {
                let (completed, ..) = ask(&object, &Counting::default());
                assert_eq!(completed, (Ok(None), vec![], 4, 0, 1));
            }
        }

        #[test]
        fn string_literal_reader_checks_bounds_and_polls_per_prefix_letter() {
            let read = |source: &[u8], code: &[u8], kind: &str, start: usize| {
                at_node(source, kind, start, |string, _| {
                    let budget = Counting::default();
                    let literal = python_string_literal(string, code, &budget)
                        .map(|literal| literal.map(|literal| (literal.interior, literal.raw)));
                    (literal, budget.steps.get(), budget.bytes.get())
                })
            };
            // expected: 4 bytes, all four letters, none of them `r`.
            let source: &[u8] = b"UuUu\"x\"\ny = 1\n";
            assert_eq!(
                read(source, source, "string", 0),
                (Ok(Some((5..6, false))), 0, 4)
            );
            // expected: 1 byte, the reading stops at the first `r`.
            let source: &[u8] = b"rrrruuuu\"x\"\ny = 1\n";
            assert_eq!(
                read(source, source, "string", 0),
                (Ok(Some((9..10, true))), 0, 1)
            );
            let source: &[u8] = b"def f():\n    \"\"\n    \"extra\"\n    return 1\n";
            assert_eq!(
                read(source, source, "string", 13),
                (Ok(Some((14..14, false))), 0, 0)
            );
            // A buffer shorter than the tree it is read with: no interior, and no panic.
            let source: &[u8] = b"def f():\n    \"audit\"\n    pass";
            assert_eq!(
                read(source, source, "string", 13),
                (Ok(Some((14..19, false))), 0, 0)
            );
            assert_eq!(read(source, &source[..15], "string", 13), (Ok(None), 0, 0));
            // A concatenation is no `string`.
            let source: &[u8] = b"def f():\n    R\"a\\nb\" u\"c\"\n";
            assert_eq!(
                read(source, source, "concatenated_string", 13),
                (Ok(None), 0, 0)
            );
        }
    }
}
