//! Validated fresh comment rendering for benchmark and test callers.
//!
//! Preparation borrows an immutable index and its ordered fragments. Each call
//! renders new text and maps through the production renderer; dropping that output
//! belongs inside a construction benchmark. Retained `complete_text` is separate.

use super::budget::{CollectionBudget, WorkBudget};
use super::text::TextError;
use super::{Composition, Content, Document, DocumentationIndex, FactError, FactRef, Fragment};
use super::{FragmentForm, SourceError, UnavailableReason};

/// Probe failures preserve identity, unsupported-form and source distinctions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// A handle came from a different index or a record is missing.
    Fact(FactError),
    /// The document has unsupported composition, fragments or content state.
    Unsupported,
    /// Source validation or active cancellation failed.
    Source(SourceError),
    /// The production renderer cannot provide complete output.
    Unavailable(UnavailableReason),
}

/// An index-borrowing prepared call to the production comment renderer.
#[derive(Debug)]
pub struct PreparedRender<'i, 's> {
    index: &'i DocumentationIndex<'s>,
    fragments: Vec<&'i Fragment<'s>>,
}

impl<'i, 's> PreparedRender<'i, 's> {
    /// Validate a real index document and retain references in composition order.
    ///
    /// # Errors
    /// Rejects foreign handles, incomplete or non-comment documents, and invalid
    /// retained source intersections. Preparation is outside the measured call.
    pub fn new(
        index: &'i DocumentationIndex<'s>,
        document: FactRef<'_, Document<'s>>,
    ) -> Result<Self, RenderError> {
        // bca: suppress(nexits, halstead) -- preparation validates identity, composition and every retained source intersection before trusting render input.
        index
            .facts
            .check_scope(document)
            .map_err(RenderError::Fact)?;
        let document = &index.facts.documents[document.ordinal];
        if !matches!(document.content, Content::Complete { .. })
            || !matches!(
                document.composition,
                Composition::Single | Composition::CommentRun
            )
            || document.fragments.is_empty()
        {
            return Err(RenderError::Unsupported);
        }
        let mut fragments = Vec::with_capacity(document.fragments.len());
        let mut previous_end = 0;
        for id in &document.fragments {
            let fragment = index
                .facts
                .fragments
                .get(id.0)
                .ok_or(RenderError::Fact(FactError::MissingRecord))?;
            if fragment.form != FragmentForm::Comment || fragment.source.has_excluded_gap() {
                return Err(RenderError::Unsupported);
            }
            let pieces = fragment.source.pieces();
            let first = pieces
                .first()
                .ok_or(RenderError::Source(SourceError::InvalidRange))?;
            let last = pieces
                .last()
                .ok_or(RenderError::Source(SourceError::InvalidRange))?;
            let range = first.range().start..last.range().end;
            if range.start < previous_end {
                return Err(RenderError::Source(SourceError::InvalidRange));
            }
            let validated = index
                .source
                .pieces(range.clone())
                .map_err(RenderError::Source)?;
            if validated != fragment.source {
                return Err(RenderError::Source(SourceError::InvalidRange));
            }
            previous_end = range.end;
            fragments.push(fragment);
        }
        Ok(Self { index, fragments })
    }

    /// Build fresh content with a fresh work budget, including an entry poll.
    ///
    /// # Errors
    /// Propagates active cancellation even for empty text, source failures and
    /// unavailable production rendering. No retained output is reused.
    pub fn render(&self) -> Result<Content<'s>, RenderError> {
        let budget = CollectionBudget::default();
        budget.checkpoint().map_err(RenderError::Source)?;
        super::text::comments::render(
            self.index.profile,
            &self.index.source,
            &self.fragments,
            &budget,
        )
        .map_err(|error| match error {
            TextError::Source(error) => RenderError::Source(error),
            TextError::Unavailable(reason) => RenderError::Unavailable(reason),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documentation::{DocumentationContext, DocumentationOptions, observations};
    use crate::{AnyParser, LANG};

    #[cfg(feature = "rust")]
    #[test]
    fn fresh_render_and_retained_access_have_distinct_work() {
        let parser =
            AnyParser::parse(LANG::Rust, b"///a\n///b\nfn f() {}".to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let document = index.documents().next().unwrap();
        let prepared = PreparedRender::new(&index, document).unwrap();
        let before = observations();
        for _ in 0..3 {
            let Content::Complete { text, mappings } = prepared.render().unwrap() else {
                panic!("complete")
            };
            assert_eq!(text, "a\nb");
            assert_eq!(
                mappings,
                index.complete_text(document).unwrap().unwrap().mappings()
            );
        }
        let after = observations();
        assert_eq!(after.renders - before.renders, 3);
        // Two copied characters and one synthetic separator per render.
        assert_eq!(after.copy_chunks - before.copy_chunks, 9);
        let retained = index.complete_text(document).unwrap().unwrap();
        for _ in 0..3 {
            let next = index.complete_text(document).unwrap().unwrap();
            assert!(std::ptr::eq(retained.text(), next.text()));
            assert!(std::ptr::eq(retained.mappings(), next.mappings()));
        }
        assert_eq!(observations().renders, after.renders);
        assert_eq!(observations().copy_chunks, after.copy_chunks);
    }

    #[cfg(feature = "rust")]
    #[test]
    fn tiny_and_empty_render_poll_cancellation_at_entry() {
        for source in ["///\nfn f() {}", "///a\nfn f() {}"] {
            let parser =
                AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let prepared = PreparedRender::new(&index, index.documents().next().unwrap()).unwrap();
            let flag = crate::cancel::ParseCancel::new();
            flag.cancel();
            let before = observations();
            assert!(matches!(
                flag.run(|| prepared.render()),
                Err(RenderError::Source(SourceError::Cancelled))
            ));
            assert_eq!(observations().renders, before.renders);
            assert!(matches!(prepared.render(), Ok(Content::Complete { .. })));
            let active = crate::cancel::ParseCancel::new();
            assert!(matches!(
                active.run(|| prepared.render()),
                Ok(Content::Complete { .. })
            ));
        }
    }

    #[cfg(feature = "rust")]
    #[test]
    fn preparation_rejects_foreign_and_pending_documents() {
        let parser = AnyParser::parse(
            LANG::Rust,
            b"#[doc(hidden)]\nfn f() {}".to_vec(),
            None,
            None,
        )
        .unwrap();
        let first = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let second = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let document = first.documents().next().unwrap();
        assert!(matches!(
            PreparedRender::new(&second, document),
            Err(RenderError::Fact(_))
        ));
        assert!(matches!(
            PreparedRender::new(&first, document),
            Err(RenderError::Unsupported)
        ));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn preparation_rejects_unavailable_and_noncomment_forms() {
        let parser =
            AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
        let mut index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        index.facts.fragments[0].form = FragmentForm::Literal;
        assert!(matches!(
            PreparedRender::new(&index, index.documents().next().unwrap()),
            Err(RenderError::Unsupported)
        ));
        index.facts.fragments[0].form = FragmentForm::Comment;
        index.facts.documents[0].content = Content::Unavailable(UnavailableReason::ExcludedGap);
        assert!(matches!(
            PreparedRender::new(&index, index.documents().next().unwrap()),
            Err(RenderError::Unsupported)
        ));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn preparation_rejects_invalid_retained_source_order() {
        let parser =
            AnyParser::parse(LANG::Rust, b"///a\n///b\nfn f() {}".to_vec(), None, None).unwrap();
        let mut index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        index.facts.documents[0].fragments.swap(0, 1);
        assert!(matches!(
            PreparedRender::new(&index, index.documents().next().unwrap()),
            Err(RenderError::Source(SourceError::InvalidRange))
        ));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn valid_preparation_borrows_adjacent_range_intersections() {
        let source = "///é\nfn f() {}".as_bytes();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANG::Rust.tree_sitter_language().unwrap())
            .unwrap();
        parser
            .set_included_ranges(&[
                tree_sitter::Range {
                    start_byte: 0,
                    end_byte: 4,
                    start_point: tree_sitter::Point::new(0, 0),
                    end_point: tree_sitter::Point::new(0, 4),
                },
                tree_sitter::Range {
                    start_byte: 4,
                    end_byte: source.len(),
                    start_point: tree_sitter::Point::new(0, 4),
                    end_point: tree_sitter::Point::new(1, 9),
                },
            ])
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let index = super::super::collect(
            LANG::Rust,
            &tree,
            source,
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
        let document = index.documents().next().unwrap();
        let prepared = PreparedRender::new(&index, document).unwrap();
        let Content::Complete { text, mappings } = prepared.render().unwrap() else {
            panic!("complete")
        };
        assert_eq!(text, "é");
        assert_eq!(
            mappings,
            index.complete_text(document).unwrap().unwrap().mappings()
        );
    }

    #[cfg(all(feature = "c", feature = "go", feature = "java"))]
    #[test]
    fn production_comment_profiles_share_the_prepared_render_seam() {
        for (language, source, expected) in [
            (LANG::C, "/**audit*/\nint a, b;", "audit"),
            (
                LANG::Go,
                "package p\n// first\n// second\nfunc F() {}",
                "first\nsecond",
            ),
            (LANG::Java, "/**\n * audit\n */\nclass C {}", "audit"),
        ] {
            let parser =
                AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            assert_eq!(index.documents().len(), 1);
            let document = index.documents().next().unwrap();
            let prepared = PreparedRender::new(&index, document).unwrap();
            let before = observations();
            let Content::Complete { text, mappings } = prepared.render().unwrap() else {
                panic!("complete")
            };
            assert_eq!(text, expected);
            assert_eq!(
                mappings,
                index.complete_text(document).unwrap().unwrap().mappings()
            );
            assert_eq!(observations().renders - before.renders, 1);
        }
    }
}
