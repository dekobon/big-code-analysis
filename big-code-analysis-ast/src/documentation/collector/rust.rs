//! Direct Rust attribute fields, assessed independently of owner projection.

use super::*;
use std::ops::Range;

// Count actual child examinations; duplicate carrier work preserves every output.
crate::observation::counter!(carrier_children, cfg(any(test, feature = "test-support")));
crate::observation::counter!(attribute_children, cfg(any(test, feature = "test-support")));

pub(super) enum Carrier {
    Other,
    Unknown,
    Doc(Value),
}

pub(super) enum Value {
    Pending,
    Boundary,
    Literal(Range<usize>),
}

#[derive(Default)]
struct AttributeFields<'s> {
    path: Option<tree_sitter::Node<'s>>,
    value: Option<tree_sitter::Node<'s>>,
    arguments: bool,
}

impl<'s> AttributeFields<'s> {
    fn read(
        canonical: &CanonicalNodes<'s>,
        attribute: NodeId,
        budget: &CollectionBudget,
    ) -> Result<Self, CollectionError> {
        let mut fields = Self::default();
        for &child in &canonical.children[canonical.nodes[attribute.0].children.clone()] {
            budget.step()?;
            attribute_children::record();
            let record = &canonical.nodes[child.0];
            let node = record.evidence.node();
            match record.field {
                Some("value") => fields.value = Some(node),
                Some("arguments") => fields.arguments = true,
                None if fields.path.is_none()
                    && node.is_named()
                    && !matches!(node.kind(), "line_comment" | "block_comment") =>
                {
                    fields.path = Some(node);
                }
                _ => {}
            }
        }
        Ok(fields)
    }

    fn value(&self, recovered: bool) -> Value {
        if recovered {
            Value::Boundary
        } else if self.arguments {
            Value::Pending
        } else {
            match self.value {
                Some(value) if matches!(value.kind(), "string_literal" | "raw_string_literal") => {
                    Value::Literal(value.byte_range())
                }
                Some(_) => Value::Pending,
                None => Value::Boundary,
            }
        }
    }
}

impl<'s> Builder<'s> {
    pub(super) fn rust_carrier(
        &self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<Carrier, CollectionError> {
        // bca: suppress(nexits, halstead) -- each uncertain path and unsupported carrier has a distinct early result.
        let canonical = &self.index.canonical;
        let item = &canonical.nodes[id.0];
        for &child in &canonical.children[item.children.clone()] {
            budget.step()?;
            carrier_children::record();
            let attribute = &canonical.nodes[child.0];
            if attribute.evidence.node().kind() != "attribute" {
                continue;
            }
            let fields = AttributeFields::read(canonical, child, budget)?;
            let Some(path) = fields.path else {
                return Ok(Carrier::Unknown);
            };
            if path.has_error()
                || path.is_missing()
                || !self
                    .index
                    .source
                    .covers_with_budget(path.byte_range(), budget)?
            {
                return Ok(Carrier::Unknown);
            }
            if path.kind() != "identifier" {
                return Ok(Carrier::Other);
            }
            let name = &self.index.source.bytes()[path.byte_range()];
            match name.strip_prefix(b"r#").unwrap_or(name) {
                b"cfg_attr" => return Ok(Carrier::Doc(Value::Pending)),
                b"doc" => {}
                _ => return Ok(Carrier::Other),
            }
            return Ok(Carrier::Doc(fields.value(item.evidence.node().has_error())));
        }
        Ok(if item.evidence.node().has_error() {
            Carrier::Unknown
        } else {
            Carrier::Other
        })
    }

    pub(super) fn rust_content(
        &self,
        draft: &Draft<'_>,
        budget: &CollectionBudget,
    ) -> Result<Content<'s>, super::super::text::TextError> {
        use super::super::text::rust::{Input, render};
        let mut pending = false;
        let mut gap = draft.gap;
        let mut boundary = false;
        let mut inputs = Vec::new();
        for &id in &draft.fragments {
            budget.step()?;
            let fragment = &self.index.facts.fragments[id.0];
            gap |= fragment.source.has_excluded_gap();
            match self.rust_values.get(&id) {
                Some(Value::Pending) => pending = true,
                Some(Value::Boundary) => boundary = true,
                Some(Value::Literal(range)) => inputs.push(Input::Literal(fragment, range.clone())),
                None => inputs.push(Input::Comment(fragment)),
            }
        }
        if pending {
            Ok(Content::PendingAdapter)
        } else if gap {
            Ok(Content::Unavailable(UnavailableReason::ExcludedGap))
        } else if boundary {
            Ok(Content::Unavailable(UnavailableReason::GrammarBoundary))
        } else {
            render(&self.index.source, &inputs, budget)
        }
    }
}
