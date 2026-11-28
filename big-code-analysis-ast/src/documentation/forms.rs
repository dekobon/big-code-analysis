//! Explicit implementation assessment for native source forms.

use super::{Profile, SourceContext};
use crate::{Ancestors, Node};

#[derive(Clone, Copy)]
pub(super) enum CommentAssessment {
    Implemented,
    Ineligible,
    Pending(&'static str),
}

pub(super) fn assess<'s, B: super::budget::WorkBudget>(
    profile: Profile,
    node: &Node<'s>,
    ancestors: Ancestors<'s, '_>,
    source: &SourceContext<'s>,
    budget: &B,
) -> Result<CommentAssessment, B::Error> {
    let parent = ancestors.parent(node).map(|node| node.kind());
    if !source.covers_with_budget(node.start_byte()..node.end_byte(), budget)? {
        return Ok(CommentAssessment::Pending(
            "Excluded bytes prevent complete native-carrier assessment",
        ));
    }
    let bytes = &source.bytes()[node.start_byte()..node.end_byte()];
    if profile == Profile::Groovydoc
        && bytes.starts_with(b"/**")
        && let Some(next) = ancestors.siblings_after_with_budget(node, budget)?.next()
        && next.start_row() == node.end_row()
    {
        return Ok(CommentAssessment::Pending(
            "Groovydoc same-row declaration placement awaits source selection",
        ));
    }
    Ok(match profile {
        Profile::Rust if matches!(parent, Some("ordered_field_declaration_list")) => {
            CommentAssessment::Pending(
                "Rust tuple-field slot projection awaits the language adapter",
            )
        }
        Profile::Kdoc
            if bytes.starts_with(b"/**")
                && matches!(
                    parent,
                    Some(
                        "class_parameters"
                            | "class_parameter"
                            | "function_value_parameters"
                            | "parameter"
                            | "primary_constructor"
                            | "modifiers"
                    )
                ) =>
        {
            CommentAssessment::Pending("KDoc signature or parameter form awaits owner projection")
        }
        Profile::BcaPhpSource
            if bytes.starts_with(b"/**")
                && matches!(
                    parent,
                    Some(
                        "formal_parameters"
                            | "property_promotion_parameter"
                            | "simple_parameter"
                            | "variadic_parameter"
                    )
                ) =>
        {
            CommentAssessment::Pending("PHP signature/promoted slot awaits owner projection")
        }
        Profile::BcaTypescriptSource
            if bytes.starts_with(b"/**")
                && matches!(
                    parent,
                    Some("formal_parameters" | "required_parameter" | "optional_parameter")
                ) =>
        {
            CommentAssessment::Pending("TypeScript parameter documentation awaits slot projection")
        }
        Profile::Ldoc if matches!(parent, Some("parameters" | "field" | "table_constructor")) => {
            CommentAssessment::Pending("Native LDoc parameter/table comment awaits adapter context")
        }
        Profile::Javadoc | Profile::Groovydoc | Profile::CsharpXml
            if matches!(
                parent,
                Some(
                    "formal_parameters"
                        | "parameter_list"
                        | "accessor_list"
                        | "accessor_declaration"
                )
            ) =>
        {
            CommentAssessment::Ineligible
        }
        _ => CommentAssessment::Implemented,
    })
}

pub(super) fn owner_pending(
    profile: Profile,
    node: &Node<'_>,
    ancestors: Ancestors<'_, '_>,
) -> bool {
    let parent = ancestors.parent(node).map(|node| node.kind());
    match profile {
        Profile::Rust => {
            matches!(parent, Some("ordered_field_declaration_list"))
                || matches!(
                    node.kind(),
                    "field_declaration" | "enum_variant" | "associated_type"
                )
        }
        Profile::Javadoc => matches!(node.kind(), "field_declaration" | "constant_declaration"),
        Profile::Groovydoc => matches!(node.kind(), "field_declaration"),
        Profile::CsharpXml => {
            matches!(node.kind(), "field_declaration" | "event_field_declaration")
        }
        Profile::Kdoc => matches!(
            node.kind(),
            "class_parameter" | "parameter" | "primary_constructor" | "property_declaration"
        ),
        Profile::BcaPhpSource => matches!(
            node.kind(),
            "property_promotion_parameter"
                | "simple_parameter"
                | "variadic_parameter"
                | "expression_statement"
                | "property_declaration"
                | "const_declaration"
        ),
        Profile::GoDoc => matches!(
            node.kind(),
            "field_declaration" | "method_elem" | "type_elem"
        ),
        Profile::Yard => matches!(node.kind(), "call" | "assignment" | "alias"),
        Profile::Ldoc => matches!(
            node.kind(),
            "assignment_statement" | "variable_declaration" | "return_statement" | "field"
        ),
        // A function owns documentation at file level and directly in the
        // braces of another function's body; any other place stays pending.
        Profile::Shdoc => match parent {
            Some("program") => false,
            Some("compound_statement") => ancestors
                .iter(node)
                .nth(1)
                .is_none_or(|(outer, _)| outer.kind() != "function_definition"),
            _ => true,
        },
        _ => false,
    }
}

pub(super) fn slot_candidate(profile: Profile, kind: &str) -> bool {
    match profile {
        Profile::Kdoc => matches!(
            kind,
            "class_parameter" | "parameter" | "primary_constructor"
        ),
        Profile::BcaPhpSource => matches!(
            kind,
            "property_promotion_parameter" | "simple_parameter" | "variadic_parameter"
        ),
        Profile::BcaTypescriptSource => matches!(kind, "required_parameter" | "optional_parameter"),
        _ => false,
    }
}
