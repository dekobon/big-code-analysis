//! Python: the aliased `block` and `lambda` kind sets.

use crate::Python;
use crate::node::Node;

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
