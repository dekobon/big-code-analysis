use super::*;

#[cfg(feature = "cpp")]
#[test]
fn one_document_shared_by_two_owners_is_not_two_alternative_claims() {
    let source = b"/**both*/\nint a, b;";
    let parser = AnyParser::parse(LANG::Cpp, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let root = context.node(parser.ts_tree().root_node()).unwrap();
    let declaration = parser
        .root_node()
        .children()
        .find(|node| node.kind() == "declaration")
        .unwrap();
    let mut facts = Facts::default();
    facts.fragments.push(Fragment {
        source: context.pieces(0..9).unwrap(),
        node: None,
        form: FragmentForm::Comment,
    });
    facts.documents.push(Document {
        fragments: vec![FragmentId(0)],
        composition: Composition::Single,
        role: DocumentRole::LeadingComment,
        content: Content::Complete {
            text: "both".into(),
            mappings: vec![],
        },
    });
    let names: Vec<_> = declaration
        .children()
        .filter(|node| node.kind() == "identifier")
        .collect();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].as_tree_sitter().byte_range(), 14..15);
    assert_eq!(names[1].as_tree_sitter().byte_range(), 17..18);
    facts.owners.push(Owner::File(root));
    facts.owners.extend(
        names
            .iter()
            .map(|node| Owner::Node(context.node(node.as_tree_sitter()).unwrap())),
    );
    for resolution in [
        Resolution::Resolved,
        Resolution::Unresolved(UnresolvedReason::Conditional),
    ] {
        facts.associations.push(Association {
            document: DocumentId(0),
            owners: vec![OwnerId(1), OwnerId(2)],
            resolution,
            convention: Convention("Doxygen"),
            binding: Some(context.node(declaration.as_tree_sitter()).unwrap()),
            lexical_scope: root,
            publication: Publication::Unknown,
        });
    }
    let associations: Vec<_> = facts.associations().collect();
    assert_eq!(facts.documents().len(), 1);
    assert_eq!(associations.len(), 2);
    assert_eq!(associations[0].get().owner_count(), 2);
    assert_eq!(associations[0].get().resolution, Resolution::Resolved);
    assert_eq!(
        associations[1].get().resolution,
        Resolution::Unresolved(UnresolvedReason::Conditional)
    );
    assert_eq!(
        facts.associated_document(associations[0]).unwrap(),
        facts.associated_document(associations[1]).unwrap()
    );
    for association in &associations {
        for (position, expected) in [14..15, 17..18].into_iter().enumerate() {
            let owner = facts.associated_owner(*association, position).unwrap();
            let Owner::Node(node) = owner.get() else {
                panic!("association must resolve its name owner, not the file decoy");
            };
            assert_eq!(node.node().byte_range(), expected);
        }
    }
    let foreign = Facts::default();
    assert_eq!(
        foreign.associated_document(associations[0]),
        Err(FactError::ForeignIndex)
    );
    assert_eq!(
        foreign.associated_owner(associations[0], 0),
        Err(FactError::ForeignIndex)
    );
    assert_eq!(
        foreign.document_order(facts.documents().next().unwrap()),
        Err(FactError::ForeignIndex)
    );
    assert_eq!(
        facts.associated_owner(associations[0], 2),
        Err(FactError::MissingRecord)
    );
    assert_eq!(
        facts.document_fragment(facts.documents().next().unwrap(), 1),
        Err(FactError::MissingRecord)
    );
}

#[cfg(feature = "typescript")]
#[test]
fn export_binding_wrapper_is_distinct_from_semantic_owner_and_lexical_scope() {
    let source = b"/**old*/\n/**audit*/\nexport function f() {}";
    let parser = AnyParser::parse(LANG::Typescript, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let wrapper = parser
        .root_node()
        .children()
        .find(|node| node.kind() == "export_statement")
        .unwrap();
    let owner = wrapper.child_by_field_name("declaration").unwrap();
    let mut facts = Facts::default();
    for (ordinal, (range, text)) in [(0..8, "old"), (9..19, "audit")].into_iter().enumerate() {
        facts.fragments.push(Fragment {
            source: context.pieces(range).unwrap(),
            node: None,
            form: FragmentForm::Comment,
        });
        facts.documents.push(Document {
            fragments: vec![FragmentId(ordinal)],
            composition: Composition::Single,
            role: DocumentRole::LeadingComment,
            content: Content::Complete {
                text: text.into(),
                mappings: vec![],
            },
        });
    }
    facts
        .owners
        .push(Owner::Node(context.node(owner.as_tree_sitter()).unwrap()));
    facts.associations.push(Association {
        document: DocumentId(1),
        owners: vec![OwnerId(0)],
        resolution: Resolution::Resolved,
        convention: Convention("BcaTypescriptSource"),
        binding: Some(context.node(wrapper.as_tree_sitter()).unwrap()),
        lexical_scope: context.node(parser.ts_tree().root_node()).unwrap(),
        publication: Publication::Unknown,
    });
    let association = facts.associations().next().unwrap();
    let documents: Vec<_> = facts.documents().collect();
    assert_eq!(documents.len(), 2);
    let selected = facts.associated_document(association).unwrap();
    assert_eq!(selected, documents[1]);
    assert_ne!(selected, documents[0]);
    assert_eq!(
        facts
            .document_fragment(selected, 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"/**audit*/"
    );
    assert_eq!(
        association.get().binding.unwrap().node().byte_range(),
        20..42
    );
    assert_eq!(association.get().lexical_scope.node().byte_range(), 0..42);
    let Owner::Node(owner) = facts.associated_owner(association, 0).unwrap().get() else {
        panic!("concrete function owner");
    };
    assert_eq!(owner.node().byte_range(), 27..42);
    assert_eq!(owner.node().kind(), "function_declaration");
}

#[cfg(feature = "rust")]
#[test]
fn slots_have_enclosing_owner_role_ordinal_and_source_without_fabricated_nodes() {
    let source = b"struct S(///first\n u32, ///second\n u32);";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let item = parser.root_node().children().next().unwrap();
    let mut facts = Facts::default();
    facts.owners.push(Owner::File(
        context.node(parser.ts_tree().root_node()).unwrap(),
    ));
    facts
        .owners
        .push(Owner::Node(context.node(item.as_tree_sitter()).unwrap()));
    for (ordinal, range) in [19..22, 35..38].into_iter().enumerate() {
        facts.owners.push(Owner::Slot(Slot {
            enclosing: OwnerId(1),
            role: SlotRole::Field,
            ordinal,
            source: context.pieces(range).unwrap(),
        }));
    }
    let owners: Vec<_> = facts.owners().collect();
    assert_eq!(owners.len(), 4);
    assert_eq!(facts.slot_enclosing(owners[2]).unwrap(), owners[1]);
    assert_eq!(facts.slot_enclosing(owners[3]).unwrap(), owners[1]);
    let Owner::Slot(slot) = owners[3].get() else {
        panic!("second slot");
    };
    assert_eq!(slot.role, SlotRole::Field);
    assert_eq!(slot.ordinal, 1);
    assert_eq!(slot.source.pieces()[0].range(), 35..38);
    assert_eq!(slot.source.pieces()[0].bytes(), b"u32");
}

#[cfg(feature = "cpp")]
#[test]
fn recovered_owner_retains_ordered_pieces_without_an_envelope_claim() {
    let source = b"/**audit*/\nvoid f() ???;";
    let parser = AnyParser::parse(LANG::Cpp, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    let context = SourceContext::new(parser.ts_tree(), source);
    let owner = Owner::Group(OwnerGroup {
        scope: context.node(parser.ts_tree().root_node()).unwrap(),
        pieces: vec![
            OwnerPiece {
                source: context.pieces(11..15).unwrap(),
                node: None,
            },
            OwnerPiece {
                source: context.pieces(16..19).unwrap(),
                node: None,
            },
        ],
    });
    let Owner::Group(group) = owner else {
        panic!("recovered group");
    };
    assert_eq!(group.pieces.len(), 2);
    assert_eq!(group.pieces[0].source.pieces()[0].range(), 11..15);
    assert_eq!(group.pieces[0].source.pieces()[0].bytes(), b"void");
    assert_eq!(group.pieces[1].source.pieces()[0].range(), 16..19);
    assert_eq!(group.pieces[1].source.pieces()[0].bytes(), b"f()");
}

#[cfg(feature = "java")]
#[test]
fn record_type_owner_does_not_become_the_synthetic_header_space() {
    use crate::{Ancestors, Checker, JavaCode};
    let source = b"/**audit*/\nrecord R(int a) {}";
    let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let record = parser
        .root_node()
        .children()
        .find(|node| node.kind() == "record_declaration")
        .unwrap();
    let header = record.child_by_field_name("parameters").unwrap();
    assert!(JavaCode::is_func_space_with_code(
        &header,
        source,
        Ancestors::unknown()
    ));
    let owner = Owner::Node(context.node(record.as_tree_sitter()).unwrap());
    let Owner::Node(node) = owner else {
        panic!("record owner");
    };
    assert_eq!(node.node().kind(), "record_declaration");
    assert_eq!(node.node().byte_range(), 11..29);
    assert_eq!(header.as_tree_sitter().byte_range(), 19..26);
}
