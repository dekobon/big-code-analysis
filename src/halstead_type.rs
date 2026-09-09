//! The operator / operand / unknown classification a `Getter` assigns
//! to each node for the Halstead metric.
//!
//! Defined beside the classifiers rather than in `metrics::halstead`
//! because `Getter::get_op_type` returns it: the parse layer names the
//! type, the metric consumes it (#1376). `metrics::halstead` re-exports
//! it under its historical public path.

/// Specifies the type of nodes accepted by the `Halstead` metric.
pub enum HalsteadType {
    /// The node is an `Halstead` operator
    Operator,
    /// The node is an `Halstead` operand
    Operand,
    /// The node is unknown to the `Halstead` metric
    Unknown,
}
