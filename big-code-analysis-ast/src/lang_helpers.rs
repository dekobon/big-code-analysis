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

pub mod bash;
// Crate-private: the doc-comment predicates are read by this crate's
// `Checker` impls; only the run walk the LOC metric drives is
// re-exported below.
pub(crate) mod doc_comment;
// Crate-private: Doxygen's attachment rule, read by the four C-family
// `Checker` impls only (#1613).
pub(crate) mod doxygen;
pub mod elixir;
// Crate-private: this dialect's only helper is a kind table the three
// classifiers in this crate share, and nothing outside names it.
pub(crate) mod irules;
pub mod perl;
pub mod python;
pub mod tcl;
// Crate-private for the same reason as `irules` above: the braced-word
// slot rule is read by this crate's three classifiers and by nothing
// outside it. The one exception is re-exported below rather than by
// widening the module, which would publish the slot tables too.
pub(crate) mod tcl_family;

/// Re-exported because both dialects' metrics need it and the module
/// holding it is crate-private: iRules' nexits and ABC walkers resolve a
/// leading word themselves rather than through `tcl::tcl_command_name`,
/// so they normalise the `::` qualifier at their own call sites.
pub use tcl_family::strip_global_qualifier;

/// Re-exported because the LOC metric bills documentation through them —
/// Go, Ruby and Lua runs by position, and the doc comments a bound
/// language's tool attaches — while the predicates beside them stay
/// crate-private.
pub use doc_comment::{
    DocBinding, KeptDocs, bound_doc_run, documents_unbound, documents_where_it_sits,
    leading_doc_run,
};
