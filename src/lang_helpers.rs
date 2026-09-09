//! Byte-level identity helpers shared by the classifiers and the metrics.
//!
//! Some questions a walk asks are not answerable from a node's kind: an
//! Elixir `def` and a `quote` are both `Call` nodes told apart only by
//! their target text, a Tcl `switch` is a plain `command` whose leading
//! word names the builtin, and Python's aliased `block` / `lambda` kinds
//! must be normalised at one site (grammar-dispatch §9 and §10). These
//! helpers read the bytes once so `Checker`, `Getter`, and every metric
//! agree on the answer.
//!
//! They live beside the classifiers rather than in a metric module
//! because the classifiers consult them: a `Checker` impl that imported
//! a helper *from* `metrics::cognitive` would make the parse layer depend
//! on the metric layer, the inversion #1376 exists to remove.

pub(crate) mod elixir;
pub(crate) mod python;
pub(crate) mod tcl;
