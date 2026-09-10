//! The role a node plays in an expression: operator, operand, or
//! neither.
//!
//! This is a syntactic classification, not a metric one. Whether a node
//! is an operator or an operand is decided by the grammar — `+` and a
//! call expression are operators, an identifier and a string literal
//! are operands — and each language's answer lives in its
//! [`Getter::get_op_type`](crate::getter::Getter::get_op_type) table
//! beside the rest of its classifiers.
//!
//! Halstead is the metric that consumes it today, and until #1376 the
//! type was named after that consumer. It is not Halstead-specific:
//! anything that reasons about operator/operand structure can use it,
//! which is why it sits in the parse layer rather than the metric.
//! `metrics::halstead` re-exports it, and keeps the old `HalsteadType`
//! spelling as a deprecated alias.

/// The role a node plays in an expression.
pub enum TokenRole {
    /// The node acts as an operator (`+`, `&&`, a call, an index).
    Operator,
    /// The node acts as an operand (an identifier, a literal).
    Operand,
    /// The node plays neither role, so it is not counted.
    Unknown,
}
