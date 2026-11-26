use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(any(
    feature = "java",
    feature = "kotlin",
    feature = "groovy",
    feature = "csharp",
    feature = "php",
    feature = "typescript",
    feature = "ruby",
    feature = "lua"
))]
fn assert_pending_form(language: LANG, source: &str, owner_kind: &str) {
    let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let node = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == owner_kind)
        .unwrap();
    let projection = index.node_projection(node.as_tree_sitter()).unwrap();
    assert!(projection.pending(), "{language:?}: {source}");
    assert!(
        projection
            .assessments()
            .any(|assessment| assessment.get().axis == AssessmentAxis::Owner)
    );
    assert_eq!(index.associations().len(), 0, "{language:?}: {source}");
    assert_eq!(index.documents().len(), 1, "{language:?}: {source}");
    let document = index.documents().next().unwrap();
    assert_eq!(document.get().fragment_count(), 1);
    assert_eq!(
        index.complete_text(document).unwrap().unwrap().text(),
        "audit"
    );
    assert!(index.assessments().any(|assessment| {
        assessment.get().axis == AssessmentAxis::Owner
            && index.assessed_document(assessment).unwrap() == Some(document)
    }));
}

#[cfg(feature = "java")]
#[test]
fn java_unnamed_field_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Java,
        "class C { /**audit*/ int _; }",
        "field_declaration",
    );
}
#[cfg(feature = "kotlin")]
#[test]
fn kotlin_parameter_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Kotlin,
        "class C(/**audit*/ val x: Int)",
        "class_parameter",
    );
}
#[cfg(feature = "groovy")]
#[test]
fn groovy_field_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Groovy,
        "class C {\n/**audit*/\nint x\n}",
        "field_declaration",
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_same_row_constructor_retains_pending_scope_assessment() {
    let parser = AnyParser::parse(
        LANG::Groovy,
        b"class C { /**audit*/ C() {} }".to_vec(),
        None,
        None,
    )
    .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 0);
    assert!(
        index
            .assessments()
            .any(|assessment| assessment.get().axis == AssessmentAxis::Selection)
    );
    let constructor = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "constructor_declaration")
        .unwrap();
    assert!(
        index
            .node_associations(constructor.as_tree_sitter())
            .unwrap()
            .assessments()
            .any(|assessment| assessment.get().axis == AssessmentAxis::Selection)
    );
}
#[cfg(feature = "csharp")]
#[test]
fn csharp_field_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Csharp,
        "class C {\n///audit\nint x;\n}",
        "field_declaration",
    );
}
#[cfg(feature = "php")]
#[test]
fn php_promoted_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Php,
        "<?php class C { function __construct(/** audit */ public int $x) {} }",
        "property_promotion_parameter",
    );
}
#[cfg(feature = "javascript")]
#[test]
fn javascript_binding_form_is_resolved_in_default_queries() {
    assert_resolved_binding_form(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_binding_form_is_resolved_in_default_queries() {
    assert_resolved_binding_form(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn assert_resolved_binding_form(language: LANG) {
    let parser = AnyParser::parse(
        language,
        b"/**audit*/\nconst f = () => 1;".to_vec(),
        None,
        None,
    )
    .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.associations().len(), 1);
    let association = index.associations().next().unwrap();
    let owner = index.facts().associated_owner(association, 0).unwrap();
    assert!(
        matches!(owner.get(), Owner::Node(node) if node.node().kind() == "variable_declarator")
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_parameter_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Typescript,
        "function f(/**audit*/ x: number) {}",
        "required_parameter",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_parameter_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Tsx,
        "function f(/**audit*/ x: number) {}",
        "required_parameter",
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_alias_forms_are_pending_in_default_queries() {
    assert_pending_form(LANG::Ruby, "# audit\nalias_method :x, :y", "call");
    assert_pending_form(LANG::Ruby, "# audit\nX = 1", "assignment");
    let source = b"# audit\nalias_method :x, :y";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let call = parser
        .root_node()
        .preorder()
        .find(|n| n.kind() == "call")
        .unwrap();
    assert!(crate::lang_helpers::ruby::ruby_documented_call(
        &call, source
    ));
    let alias = b"class C\n# audit\nalias b a\nend";
    let parser = AnyParser::parse(LANG::Ruby, alias.to_vec(), None, None).unwrap();
    let node = parser
        .root_node()
        .preorder()
        .find(|n| n.kind() == "alias")
        .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert!(
        index
            .node_projection(node.as_tree_sitter())
            .unwrap()
            .pending()
    );
}
#[cfg(feature = "lua")]
#[test]
fn lua_assignment_form_is_pending_in_default_queries() {
    assert_pending_form(
        LANG::Lua,
        "module(\"m\")\n---audit\nvalue = 1",
        "assignment_statement",
    );
}

#[cfg(feature = "c")]
#[test]
fn multi_slot_projection_exposes_each_owner_and_its_shared_document() {
    let parser = AnyParser::parse(LANG::C, b"/**audit*/\nint a, b;".to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let declaration = parser.root_node().children().last().unwrap();
    let projection = index.node_projection(declaration.as_tree_sitter()).unwrap();
    assert!(!projection.pending());
    assert_eq!(projection.owners().len(), 2);
    for owner in projection.owners() {
        let query = index.owner_associations(owner).unwrap();
        assert_eq!(query.associations().len(), 1);
        assert_eq!(
            index
                .facts()
                .associated_document(query.associations().next().unwrap())
                .unwrap(),
            index.documents().next().unwrap()
        );
    }
}
