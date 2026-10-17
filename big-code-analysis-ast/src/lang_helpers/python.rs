//! Python: the aliased `block` and `lambda` kind sets, and the shape
//! of a docstring.

use crate::Python;
use crate::lang_helpers::doc_comment::node_bytes;
use crate::node::{AncestorIter, Ancestors, Node};

/// Whether `node` is a Python `lambda` expression, under either of the
/// grammar's two aliased kind_ids: `Lambda` (196, the concrete
/// production emitted today) and `Lambda2` (197, the currently-unseen
/// hidden alias). `Lambda3` (73) is the `lambda` *keyword* token, not a
/// closure node, and is intentionally excluded.
///
/// This is the single normalization chokepoint for the lambda-alias set
/// — mirroring [`python_is_block`] for the block aliases (#419). It
/// is reused by `PythonCode::is_closure` here and by the cognitive
/// lambda-scope walks in `big-code-analysis`, so a future grammar bump
/// that promotes `Lambda2` to a concrete node is handled in exactly one
/// place rather than drifting across sites (#422). The
/// `python_hidden_block_and_lambda_aliases_stay_unseen` drift guard in
/// `checker.rs` trips on such a bump.
#[inline]
#[must_use]
pub fn python_is_lambda(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Lambda | Python::Lambda2)
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

/// Whether `node` is the sole child of an `expression_statement`: the
/// discarded-value shape a docstring takes. `"a", "b"` has two, so it
/// is a discarded tuple, not documentation.
#[inline]
#[must_use]
pub fn python_is_bare_statement_value(node: &Node, ancestors: Ancestors<'_, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| is_bare_statement(&parent))
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
/// [`python_is_documentation_string`] (#1597).
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

/// Whether the bare string statement whose value is `value` (a `string`
/// or `concatenated_string`, per [`python_is_bare_statement_value`]) sits
/// where Python tooling reads documentation (#1597):
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
#[must_use]
pub fn python_is_documentation_string(
    value: &Node,
    code: &[u8],
    ancestors: Ancestors<'_, '_>,
) -> bool {
    // The literal test is the cheap one; `body_attributes` may read the
    // enclosing `__init__`'s name and parameters.
    if !is_str_literal(value, code) {
        return false;
    }
    let mut up = ancestors.iter(value);
    let Some((statement, statement_ancestors)) = up.next() else {
        return false;
    };
    let Some(attributes) = body_attributes(up, code) else {
        return false;
    };
    let previous = statement_ancestors
        .siblings_before(&statement)
        .filter(|sibling| sibling.is_named() && sibling.kind_id() != Python::Comment)
        .find(|sibling| !is_string_statement(sibling, code));
    previous.is_none_or(|previous| attributes.documented_by(&previous, code))
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
    let Some((statement, outer)) = ancestors.iter(value).next() else {
        return;
    };
    if outer
        .siblings_after(&statement)
        .find(|sibling| sibling.is_named() && sibling.kind_id() != Python::Comment)
        .is_some_and(|next| is_string_statement(&next, code))
        || !python_is_documentation_string(value, code, ancestors)
    {
        return;
    }
    let first = outer
        .siblings_before(&statement)
        .filter(|sibling| sibling.is_named() && sibling.kind_id() != Python::Comment)
        .take_while(|sibling| is_string_statement(sibling, code))
        .last()
        .unwrap_or(statement);
    for member in std::iter::once(first)
        .chain(outer.siblings_after(&first))
        .take_while(|member| member.start_byte() <= statement.start_byte())
        .filter(|member| is_string_statement(member, code))
    {
        if let Some(value) = member.child(0) {
            credit(value.start_row(), value.end_row());
        }
    }
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
        // A `type X = …` alias (PEP 695) declares the name it binds, and
        // Sphinx's `VariableCommentPicker` reads it like an assignment
        // (`AssignmentLikeType` holds `ast.TypeAlias`): a docstring after
        // it or a `#:` comment on or above it documents `X`. It never
        // binds an attribute of `self`.
        if statement.kind_id() == Python::TypeAliasStatement {
            return matches!(self, Self::Names);
        }
        let Some(target) = assignment_target(statement) else {
            return false;
        };
        match self {
            Self::Names => binds(&target, |leaf| leaf.kind_id() == Python::Identifier),
            Self::OfSelf(name) => binds(&target, |leaf| {
                leaf.kind_id() == Python::Attribute
                    && leaf.child_by_field_name("object").is_some_and(|object| {
                        object.kind_id() == Python::Identifier && node_bytes(&object, code) == *name
                    })
            }),
            Self::Nothing => false,
        }
    }
}

/// What a body makes of the assignments in it, from the ancestors above
/// the statement: `None` when the body holds no documentation at all (an
/// `if` or loop body).
fn body_attributes<'code>(
    mut up: AncestorIter<'_, '_>,
    code: &'code [u8],
) -> Option<Attributes<'code>> {
    let (container, _) = up.next()?;
    if container.kind_id() == Python::Module {
        return Some(Attributes::Names);
    }
    let (owner, _) = up.next().filter(|_| python_is_block(&container))?;
    owner_attributes(&owner, up, code)
}

/// What the body of `owner` — a class or function definition — makes of
/// the assignments in it; `None` for any other owner. `up` continues
/// from `owner`'s parent.
fn owner_attributes<'code>(
    owner: &Node,
    up: AncestorIter<'_, '_>,
    code: &'code [u8],
) -> Option<Attributes<'code>> {
    if owner.kind_id() == Python::ClassDefinition {
        Some(Attributes::Names)
    } else if owner.kind_id() == Python::FunctionDefinition {
        let receiver = is_class_init(owner, up, code)
            .then(|| first_parameter_name(owner, code))
            .flatten();
        Some(receiver.map_or(Attributes::Nothing, Attributes::OfSelf))
    } else {
        None
    }
}

/// Whether `function` is a class's `__init__`, the one method PEP 257
/// reads attribute docstrings in: named so, with the class the first
/// ancestor past its decorators and the class body's block. `up`
/// continues from `function`'s parent.
fn is_class_init(function: &Node, up: AncestorIter<'_, '_>, code: &[u8]) -> bool {
    function
        .child_by_field_name("name")
        .is_some_and(|name| node_bytes(&name, code) == b"__init__")
        && up
            .map(|(node, _)| node)
            .find(|node| node.kind_id() != Python::DecoratedDefinition && !python_is_block(node))
            .is_some_and(|class| class.kind_id() == Python::ClassDefinition)
}

/// The name of `function`'s first parameter, typed or not. A comment is a
/// named extra, and formatters put one right after the `(` of an exploded
/// signature (`def __init__(  # noqa: PLR0913`), so it is skipped.
fn first_parameter_name<'code>(function: &Node, code: &'code [u8]) -> Option<&'code [u8]> {
    let first = function
        .child_by_field_name("parameters")?
        .children()
        .find(|child| child.is_named() && child.kind_id() != Python::Comment)?;
    let name = if first.kind_id() == Python::TypedParameter {
        first.children().find(Node::is_named)?
    } else {
        first
    };
    (name.kind_id() == Python::Identifier).then(|| node_bytes(&name, code))
}

/// Whether `value` is a literal Python stores as a docstring: a `str`, not
/// an f-string, a template string or bytes — a prefix holding `f`, `t` or
/// `b` in either case makes the statement none, and one fragment of an
/// implicit concatenation is enough.
fn is_str_literal(value: &Node, code: &[u8]) -> bool {
    let is_str = |literal: &Node| {
        !node_bytes(literal, code)
            .iter()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .any(|byte| matches!(byte.to_ascii_lowercase(), b'f' | b't' | b'b'))
    };
    if value.kind_id() == Python::ConcatenatedString {
        value
            .children()
            .filter(|fragment| fragment.kind_id() == Python::String)
            .all(|fragment| is_str(&fragment))
    } else {
        is_str(value)
    }
}

/// An `expression_statement` holding one value and nothing else: the
/// discarded-value shape a docstring takes.
fn is_bare_statement(statement: &Node) -> bool {
    statement.kind_id() == Python::ExpressionStatement && statement.child_count() == 1
}

/// A bare statement whose value is a `str` literal: a docstring, when it
/// opens its body.
fn is_string_statement(statement: &Node, code: &[u8]) -> bool {
    is_bare_statement(statement)
        && statement.child(0).is_some_and(|value| {
            matches!(
                value.kind_id().into(),
                Python::String | Python::ConcatenatedString
            ) && is_str_literal(&value, code)
        })
}

/// Whether an assignment target binds what Sphinx records an attribute
/// docstring for, as its `get_lvar_names` reads a target: `leaf` decides a
/// single one — a name, a subscript or an attribute — and a tuple, list or
/// starred target binds when any element does. Walked with an explicit
/// stack, so a target nested thousands of brackets deep cannot exhaust the
/// thread's stack (#1597).
fn binds(target: &Node, leaf: impl Fn(&Node) -> bool) -> bool {
    let mut pending = vec![*target];
    while let Some(node) = pending.pop() {
        match node.kind_id().into() {
            Python::Identifier | Python::Subscript | Python::Attribute => {
                if leaf(&node) {
                    return true;
                }
            }
            _ => pending.extend(node.children().filter(Node::is_named)),
        }
    }
    false
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
