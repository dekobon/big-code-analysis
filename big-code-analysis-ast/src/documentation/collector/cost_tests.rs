use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(feature = "rust")]
#[test]
fn deep_and_wide_source_walks_visit_every_node_once() {
    for size in [40, 80, 160] {
        for source in [
            format!(
                "{}///audit\nfn f() {{}}{}",
                "mod m {".repeat(size),
                "}".repeat(size)
            ),
            "///audit\nfn f() {}\n".repeat(size),
        ] {
            let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
            let nodes = parser.root_node().preorder().count();
            let before = observations();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let after = observations();
            assert_eq!(after.visits - before.visits, nodes);
            assert_eq!(after.child_batches - before.child_batches, nodes);
            assert_eq!(
                after.target_classifications - before.target_classifications,
                nodes - 1
            );
            assert_eq!(
                after.target_computations - before.target_computations,
                nodes - 1
            );
            assert_eq!(after.renders - before.renders, index.documents().len());
        }
    }
}

#[cfg(feature = "c")]
#[test]
fn owner_fanout_does_not_multiply_rendered_text_or_mapping_storage() {
    for width in [2, 40, 80] {
        let names = (0..width)
            .map(|at| format!("v{at}"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("/**{}*/\nint {names};", "audit".repeat(400));
        let parser = AnyParser::parse(LANG::C, source.into_bytes(), None, None).unwrap();
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        let text = index.complete_text(document).unwrap().unwrap();
        assert_eq!(text.text().len(), 2000);
        assert_eq!(text.mappings().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().owner_count(), width);
        assert_eq!(observations().renders - before.renders, 1);
        assert_eq!(observations().copy_chunks - before.copy_chunks, 1);
    }
}

#[cfg(feature = "rust")]
#[test]
fn pending_scope_relations_are_shared_instead_of_copied_per_owner() {
    let source = format!("#![doc(hidden)]\n{}", "fn f() {}\n".repeat(300));
    let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.assessments().len(), 3);
    let node = parser.root_node().children().last().unwrap();
    let query = index.node_associations(node.as_tree_sitter()).unwrap();
    assert_eq!(query.associations().count(), 0);
    assert_eq!(query.assessments().count(), 2);
}
