//! Exact legacy intercrate event vocabulary. Not a stable documentation API.
//! Consumers retain event order and apply their existing row corrections;
//! source selection cannot be substituted for this stream after binding.

use crate::Node;

/// A legacy recognition/coverage event, emitted at the original callback point.
/// Root LOC retains row sets, physical space attribution and aggregation.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum LegacyEvent<'t> {
    /// Marker and placement answers for the current comment. Keeping them
    /// separate permits Rust's marker-dependent newline correction at the root.
    Comment {
        /// The original syntax node, including any grammar-owned terminator.
        node: Node<'t>,
        /// Exact legacy marker answer, before binding selection.
        marker: bool,
        /// Exact unbound/placement decision, independently of source owners.
        by_placement: bool,
    },
    /// A bound/gathered comment, in the existing callback order. The root applies
    /// its language-specific row span rather than a fragment spelling span.
    BoundComment(Node<'t>),
    /// Original POD/data row classification inside an opaque Perl node.
    PerlDataRows(crate::lang_helpers::perl::PerlDataSectionRows),
    /// Exact inclusive rows from a positional callback, including Python
    /// parentheses. These are not reconstructed from cleaned text.
    Coverage {
        /// Which original callback supplied the rows.
        kind: CoverageKind,
        /// First zero-based source row, inclusive.
        first: usize,
        /// Last zero-based source row, inclusive.
        last: usize,
    },
}

/// The positional callback that produced legacy coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoverageKind {
    /// Go/Ruby/Lua comment-run callback.
    LeadingRun,
    /// Python primary/additional/attribute-string callback.
    PythonStringRun,
    /// Python Sphinx attribute-comment callback.
    SphinxComment,
}

// The marker/positional migrations wire these private requests into the shared
// scan. Keeping the lazy sink concrete here pins zero extra materialization
// without changing existing callbacks before their atomic root migration.
#[allow(dead_code)]
pub(crate) mod evidence {
    use super::super::{Convention, NodeEvidence};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) enum View {
        Legacy,
        Source(Convention),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Question {
        Candidate,
        Placement,
        Binding,
    }

    #[derive(Clone, Debug)]
    pub(crate) struct SelectionKey<'s> {
        pub node: NodeEvidence<'s>,
        pub view: View,
        pub question: Question,
    }

    impl PartialEq for SelectionKey<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.view == other.view
                && self.question == other.question
                && self.node.validate(other.node.node()).is_ok()
        }
    }
    impl Eq for SelectionKey<'_> {}

    pub(crate) enum Request<'a, E> {
        LegacyOnly,
        Source {
            retain_unbound: bool,
            sink: &'a mut dyn FnMut(E),
        },
    }

    impl<E> Request<'_, E> {
        pub(crate) fn emit(&mut self, selected: bool, build: impl FnOnce() -> E) {
            if let Self::Source {
                retain_unbound,
                sink,
            } = self
                && (selected || *retain_unbound)
            {
                sink(build());
            }
        }
    }
}
