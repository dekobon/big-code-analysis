//! Python docstring text: every fragment decoded into one value, which is cleaned once.

use super::literals::{LiteralPolicy, decode};
use super::{TextBuilder, TextError, cleanup};
use crate::Node;
use crate::documentation::budget::WorkBudget;
use crate::documentation::byte_scan;
use crate::documentation::{Content, Fragment, SourceContext, SourceError, UnavailableReason};
use crate::lang_helpers::python::python_string_literal;

/// The content of one docstring document from its fragments, in this order: pending
/// for a carriage return in an interior, unavailable for an excluded gap in the
/// value, unavailable for an interior that cannot be read, and otherwise the decoded
/// interiors joined with nothing between them and cleaned as one value.
pub(in crate::documentation) fn render<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    // bca: suppress(abc, halstead, nexits) -- the text states in one fixed order; reads can fail.
    let source = |error: B::Error| TextError::Source(error.into());
    let mut literals = Vec::with_capacity(fragments.len());
    let mut undefined = false;
    for fragment in fragments {
        budget.step().map_err(source)?;
        let literal = match fragment.node() {
            Some(evidence) => python_string_literal(
                &Node::from_tree_sitter(evidence.node()),
                context.bytes(),
                budget,
            )
            .map_err(source)?,
            None => None,
        };
        if let Some(literal) = &literal
            && !fragment.source().has_excluded_gap()
            && let Some(interior) = context.bytes().get(literal.interior.clone())
            && byte_scan::position(interior, |byte| byte == b'\r', budget)
                .map_err(source)?
                .is_some()
        {
            return Ok(Content::PendingAdapter);
        }
        undefined |= literal.is_none();
        literals.push(literal);
    }
    // The value from the start of its first fragment to the end of its last. A
    // document has a fragment and a fragment has a piece, so no parsed source gives
    // the error.
    let (Some(first), Some(last)) = (
        fragments
            .first()
            .and_then(|fragment| fragment.source().pieces().first()),
        fragments
            .last()
            .and_then(|fragment| fragment.source().pieces().last()),
    ) else {
        return Err(TextError::Source(SourceError::Excluded));
    };
    if !context
        .covers_with_budget(first.range().start..last.range().end, budget)
        .map_err(source)?
    {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    }
    if undefined {
        return Err(TextError::Unavailable(UnavailableReason::GrammarBoundary));
    }
    let mut builder = TextBuilder::default();
    // After the test above every interior is defined, so `flatten` skips nothing.
    for literal in literals.iter().flatten() {
        budget.step().map_err(source)?;
        let policy = if literal.raw {
            LiteralPolicy::Raw
        } else {
            LiteralPolicy::PythonCooked
        };
        decode(
            context,
            literal.interior.clone(),
            policy,
            &mut builder,
            budget,
        )?;
    }
    cleanup::python(builder.finish(), budget)
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use super::*;
    use crate::documentation::FragmentForm;
    use crate::{AnyParser, LANG};
    use std::cell::Cell;

    /// Counts `step` calls and sums `bytes` charges.
    #[derive(Default)]
    struct Counting {
        steps: Cell<usize>,
        bytes: Cell<usize>,
    }

    impl WorkBudget for Counting {
        type Error = SourceError;

        fn checkpoint(&self) -> Result<(), SourceError> {
            Ok(())
        }

        fn step(&self) -> Result<(), SourceError> {
            self.steps.set(self.steps.get() + 1);
            Ok(())
        }

        fn bytes(&self, count: usize) -> Result<(), SourceError> {
            self.bytes.set(self.bytes.get() + count);
            Ok(())
        }
    }

    #[test]
    fn python_render_charges_each_fragment_twice_and_every_scan_it_starts() {
        for fragments in [1, 3, 300] {
            // A module docstring of raw literals `r"a"`, joined by one space.
            let source = format!("{}\nx = 1\n", vec!["r\"a\""; fragments].join(" "));
            let parser =
                AnyParser::parse(LANG::Python, source.clone().into_bytes(), None, None).unwrap();
            let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
            let value = parser
                .ts_tree()
                .root_node()
                .child(0)
                .unwrap()
                .child(0)
                .unwrap();
            let mut cursor = value.walk();
            let nodes: Vec<_> = if fragments == 1 {
                vec![value]
            } else {
                value.children(&mut cursor).collect()
            };
            assert_eq!(nodes.len(), fragments);
            assert!(nodes.iter().all(|node| node.kind() == "string"));
            let owned: Vec<_> = nodes
                .iter()
                .map(|node| Fragment {
                    source: context.pieces(node.byte_range()).unwrap(),
                    node: Some(context.node(*node).unwrap()),
                    form: FragmentForm::Literal,
                })
                .collect();
            let borrowed: Vec<_> = owned.iter().collect();

            // The scans `render` starts, each run here under a budget of its own: the
            // literal reader, the carriage-return scan and the decoder for each
            // fragment, the coverage of the value, and the cleanup of the joined text.
            let parts = Counting::default();
            let mut builder = TextBuilder::default();
            for node in &nodes {
                let literal =
                    python_string_literal(&Node::from_tree_sitter(*node), context.bytes(), &parts)
                        .unwrap()
                        .unwrap();
                assert!(literal.raw);
                let interior = &context.bytes()[literal.interior.clone()];
                assert_eq!(
                    byte_scan::position(interior, |byte| byte == b'\r', &parts),
                    Ok(None)
                );
                decode(
                    &context,
                    literal.interior,
                    LiteralPolicy::Raw,
                    &mut builder,
                    &parts,
                )
                .unwrap();
            }
            let envelope = nodes[0].start_byte()..nodes[fragments - 1].end_byte();
            assert_eq!(context.covers_with_budget(envelope, &parts), Ok(true));
            let Content::Complete { text: expected, .. } =
                cleanup::python(builder.finish(), &parts).unwrap()
            else {
                panic!("complete text")
            };
            assert_eq!(expected, "a".repeat(fragments));
            // The control: each of the scans charged something.
            // expected: at least one byte for each prefix `r`, one for each scanned
            // interior byte and one for each decoded byte.
            assert!(parts.bytes.get() >= 3 * fragments);
            assert_ne!(parts.steps.get(), 0);

            let whole = Counting::default();
            let Content::Complete { text, .. } = render(&context, &borrowed, &whole).unwrap()
            else {
                panic!("complete text")
            };
            assert_eq!(text, expected);
            // expected: the scans above, and one step for each fragment in each of the
            // two loops of `render`. No third loop over the fragments is charged.
            assert_eq!(whole.steps.get(), parts.steps.get() + 2 * fragments);
            // expected: `render` reads no byte itself.
            assert_eq!(whole.bytes.get(), parts.bytes.get());
        }
    }
}
