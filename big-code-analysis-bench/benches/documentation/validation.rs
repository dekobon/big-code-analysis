//! Untimed semantic and grammar-shape checks for the documentation workloads.

use big_code_analysis_ast::AnyParser;
use big_code_analysis_ast::documentation::{
    AssessmentAxis, Content, DocumentationContext, DocumentationIndex, DocumentationOptions,
    OriginView, Owner, Resolution, UnresolvedReason,
};

pub(super) fn documents(
    index: &DocumentationIndex<'_>,
    count: usize,
    fragments: usize,
    owners: usize,
    expected: &str,
) {
    assert_eq!(index.documents().len(), count);
    assert_eq!(index.associations().len(), count);
    for association in index.associations() {
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().owner_count(), owners);
        let document = index
            .facts()
            .associated_document(association)
            .unwrap_or_else(|error| panic!("document: {error:?}"));
        assert_eq!(document.get().fragment_count(), fragments);
        let text = index
            .complete_text(document)
            .unwrap_or_else(|error| panic!("text: {error:?}"))
            .unwrap_or_else(|| panic!("complete text"));
        assert_eq!(text.text(), expected);
        let mut end = 0;
        for portion in text
            .origins(0..expected.len())
            .unwrap_or_else(|error| panic!("map: {error:?}"))
        {
            assert_eq!(portion.output.start, end);
            end = portion.output.end;
            if let OriginView::Copied(piece) = portion.origin {
                assert_eq!(piece.bytes(), &expected.as_bytes()[portion.output]);
            }
        }
        assert_eq!(end, expected.len());
        for position in 0..owners {
            let owner = index
                .facts()
                .associated_owner(association, position)
                .unwrap_or_else(|error| panic!("owner: {error:?}"));
            let query = index
                .owner_associations(owner)
                .unwrap_or_else(|error| panic!("query: {error:?}"));
            assert_eq!(query.associations().collect::<Vec<_>>(), [association]);
        }
    }
}

fn shape(name: &str, size: usize, parser: &AnyParser) {
    let root = parser.root_node();
    assert_eq!(
        root.has_error(),
        matches!(name, "wide-error" | "missing"),
        "{name}"
    );
    let nodes: Vec<_> = root.preorder().collect();
    let count = |kind| nodes.iter().filter(|node| node.kind() == kind).count();
    let mut depth = 0;
    let mut cursor = parser.ts_tree().walk();
    let mut maximum = 0;
    loop {
        maximum = maximum.max(depth);
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break;
            }
            depth -= 1;
        }
        if depth == 0 {
            break;
        }
    }
    match name {
        "depth" => {
            assert_eq!(count("mod_item"), size);
            assert!(maximum >= 2 * size);
        }
        "width" => assert_eq!(root.child_count(), size * 2),
        "fragments" => assert_eq!(count("comment"), size),
        "c-slots" | "fanout-fixed-text" => assert_eq!(count("identifier"), size),
        "visible-declarator-depth" => {
            assert_eq!(count("array_declarator"), size);
            assert!(maximum >= size);
        }
        "ruby-modifiers" => {
            assert_eq!(count("if_modifier"), size);
            assert!(maximum >= size);
        }
        "java-undocumented" => {
            assert_eq!(count("class_declaration"), size);
            assert!(maximum >= 2 * size);
        }
        "pending-scope" => assert_eq!(count("function_item"), size),
        "names" => assert_eq!(count("function_definition"), size),
        "missing" => {
            assert_eq!(nodes.iter().filter(|node| node.is_missing()).count(), 1);
            assert!(maximum >= size);
        }
        "wide-error" => {
            let error = nodes
                .iter()
                .find(|node| node.is_error())
                .unwrap_or_else(|| panic!("wide ERROR"));
            assert!(error.child_count() >= size);
            assert_eq!(count("field_declaration"), size);
        }
        _ => assert!(matches!(
            name,
            "single-line-lua"
                | "single-line-jsdoc"
                | "single-line-php"
                | "single-line-ruby"
                | "single-line-doxygen"
                | "text-fixed-fanout"
        )),
    }
}

#[allow(clippy::too_many_lines)] // One explicit expected outcome per benchmark workload.
pub(super) fn fixture(
    name: &str,
    size: usize,
    parser: &AnyParser,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    shape(name, size, parser);
    let index = parser
        .documentation(context, &options)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    match name {
        "depth" | "ruby-modifiers" | "visible-declarator-depth" => {
            documents(&index, 1, 1, 1, "audit");
        }
        "width" => documents(&index, size, 1, 1, "audit"),
        "fragments" => documents(
            &index,
            1,
            size,
            1,
            &std::iter::repeat_n("audit", size)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        "c-slots" => documents(&index, 1, 1, size, "audit"),
        "single-line-lua" | "single-line-ruby" => {
            documents(&index, 1, 1, 1, &"audit ".repeat(size));
        }
        "single-line-jsdoc" | "single-line-php" | "single-line-doxygen" => {
            documents(&index, 1, 1, 1, &"audit ".repeat(size)[..size * 6 - 1]);
        }
        "text-fixed-fanout" => documents(&index, 1, 1, 2, &"audit ".repeat(size)[..size * 6 - 1]),
        "fanout-fixed-text" => documents(&index, 1, 1, size, &"audit ".repeat(1024)[..6143]),
        "java-undocumented" => {
            assert_eq!(index.documents().len(), 0);
            assert_eq!(index.facts().owners().len(), size + 2);
        }
        "missing" => {
            assert_eq!(index.documents().len(), 0);
            assert_eq!(index.associations().len(), 0);
        }
        "wide-error" => {
            assert_eq!(index.documents().len(), 1);
            let association = index
                .associations()
                .next()
                .unwrap_or_else(|| panic!("recovery association"));
            assert_eq!(
                association.get().resolution,
                Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
            );
            assert_eq!(association.get().owner_count(), 0);
        }
        "pending-scope" => {
            assert_eq!(index.documents().len(), 1);
            let document = index
                .documents()
                .next()
                .unwrap_or_else(|| panic!("pending attribute"));
            assert!(matches!(document.get().content(), Content::PendingAdapter));
            assert_eq!(index.assessments().len(), 3);
            let last = parser
                .root_node()
                .children()
                .last()
                .unwrap_or_else(|| panic!("last function"));
            let query = index
                .node_associations(last.as_tree_sitter())
                .unwrap_or_else(|error| panic!("query: {error:?}"));
            assert_eq!(query.associations().len(), 0);
            assert_eq!(
                query
                    .assessments()
                    .map(|assessment| assessment.get().axis)
                    .collect::<Vec<_>>(),
                [AssessmentAxis::Occurrence, AssessmentAxis::Publication]
            );
        }
        "names" => {
            assert_eq!(index.documents().len(), size);
            assert_eq!(index.associations().len(), size);
            for (ordinal, association) in index.associations().enumerate() {
                assert_eq!(association.get().resolution, Resolution::Resolved);
                assert_eq!(association.get().owner_count(), 1);
                let owner = index
                    .facts()
                    .associated_owner(association, 0)
                    .unwrap_or_else(|error| panic!("owner: {error:?}"));
                let Owner::Node(node) = owner.get() else {
                    panic!("function definition")
                };
                assert_eq!(
                    &parser.code()[node.node().byte_range()],
                    format!("void f{ordinal}() {{}}").as_bytes()
                );
                let document = index
                    .facts()
                    .associated_document(association)
                    .unwrap_or_else(|error| panic!("document: {error:?}"));
                assert_eq!(
                    index
                        .complete_text(document)
                        .unwrap_or_else(|error| panic!("text: {error:?}"))
                        .unwrap_or_else(|| panic!("complete text"))
                        .text(),
                    format!("@fn void f{ordinal}()")
                );
            }
        }
        _ => panic!("unvalidated workload {name}"),
    }
}
