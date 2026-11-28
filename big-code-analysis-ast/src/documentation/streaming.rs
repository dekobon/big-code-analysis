//! Exact legacy intercrate event vocabulary. Not a stable documentation API.
//! Consumers retain event order and apply their existing row corrections;
//! source selection cannot be substituted for this stream after binding.

use crate::Node;

pub mod markers;
pub mod positional;

/// Stream a positional comment and its completed run's original inclusive rows.
/// Coverage follows the current comment's billing and excludes directive members.
pub fn positional_comment<'t, T: crate::Checker>(
    node: &Node<'t>,
    code: &[u8],
    ancestors: crate::Ancestors<'t, '_>,
    mut sink: impl FnMut(LegacyEvent<'t>),
) {
    marker_comment::<T>(node, code, ancestors, &mut sink);
    crate::lang_helpers::leading_doc_run::<T>(node, code, ancestors, |first, last| {
        sink(LegacyEvent::Coverage {
            kind: CoverageKind::LeadingRun,
            first,
            last,
        });
    });
}

/// Stream the original marker/placement decision before the final-member binding
/// callbacks. This path needs no source context and constructs no source evidence.
pub fn marker_comment<'t, T: crate::Checker>(
    node: &Node<'t>,
    code: &[u8],
    ancestors: crate::Ancestors<'t, '_>,
    mut sink: impl FnMut(LegacyEvent<'t>),
) {
    use crate::lang_helpers::doc_comment::{
        bound_doc_run, documents_unbound, documents_where_it_sits,
    };
    let marker = markers::legacy_marker::<T>(node, code, ancestors);
    let by_placement = marker
        && documents_unbound::<T>(node, code, ancestors)
        && documents_where_it_sits::<T>(node, code, ancestors);
    sink(LegacyEvent::Comment {
        node: *node,
        marker,
        by_placement,
    });
    bound_doc_run::<T>(node, code, ancestors, |member| {
        sink(LegacyEvent::BoundComment(*member));
    });
}

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

// Generic request/key primitives for positional adapters. Marker streaming uses
// the typed SourceRequest and separate visited-node slots in `markers`.
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
