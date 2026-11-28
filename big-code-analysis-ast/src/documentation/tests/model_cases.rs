use super::*;

#[cfg(feature = "ruby")]
#[test]
fn file_and_virtual_owners_retain_distinct_source_evidence() {
    let source = b"# @!method ghost\n#   audit\n";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let facts = Facts {
        owners: vec![
            Owner::File(context.node(parser.ts_tree().root_node()).unwrap()),
            Owner::Virtual(VirtualOwner {
                name: context.pieces(11..16).unwrap(),
                declaration: context.pieces(2..16).unwrap(),
            }),
        ],
        ..Facts::default()
    };
    let owners: Vec<_> = facts.owners().collect();
    assert_eq!(owners.len(), 2);
    let Owner::File(file) = owners[0].get() else {
        panic!("file owner");
    };
    assert_eq!(file.node().byte_range(), 0..27);
    let Owner::Virtual(virtual_owner) = owners[1].get() else {
        panic!("virtual owner");
    };
    assert_eq!(virtual_owner.name.pieces()[0].range(), 11..16);
    assert_eq!(virtual_owner.name.pieces()[0].bytes(), b"ghost");
    assert_eq!(
        virtual_owner.declaration.pieces()[0].bytes(),
        b"@!method ghost"
    );
}

#[cfg(feature = "php")]
#[test]
fn rejected_target_and_ordinary_doc_kind_remain_raw_evidence() {
    use crate::{Ancestors, Checker, PhpCode, roles::php};
    let source = b"<?php\n/**/\nunknown();";
    let parser = AnyParser::parse(LANG::Php, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let comment = parser
        .root_node()
        .children()
        .find(|node| PhpCode::is_comment(node))
        .unwrap();
    let statement = parser
        .root_node()
        .children()
        .find(|node| php::doc_target::lists_kind(node.kind_id()))
        .unwrap();
    assert!(php::doc_comment::lists_kind(comment.kind_id()));
    assert!(!PhpCode::is_doc_comment(&comment, source));
    assert!(!PhpCode::is_doc_target(
        &statement,
        source,
        Ancestors::unknown()
    ));
    let fragment = Fragment {
        source: context.pieces(6..10).unwrap(),
        node: Some(context.node(comment.as_tree_sitter()).unwrap()),
        form: FragmentForm::Comment,
    };
    assert_eq!(fragment.source().pieces()[0].bytes(), b"/**/");
    assert_eq!(
        context
            .node(statement.as_tree_sitter())
            .unwrap()
            .node()
            .byte_range(),
        11..21
    );
}

#[cfg(feature = "tcl")]
#[test]
fn source_callable_owner_does_not_require_metric_space_promotion() {
    use crate::{Ancestors, Checker, TclCode};
    let source = b"oo::class create C {\nmethod f {} {\n# audit\nreturn\n}\n}";
    let parser = AnyParser::parse(LANG::Tcl, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let method = parser
        .root_node()
        .preorder()
        .find(|node| node.start_byte() == 21 && TclCode::is_call(node))
        .unwrap();
    assert!(!TclCode::is_func_space_with_code(
        &method,
        source,
        Ancestors::unknown()
    ));
    let owner = Owner::Node(context.node(method.as_tree_sitter()).unwrap());
    let Owner::Node(node) = owner else {
        panic!("explicit method owner");
    };
    assert_eq!(node.node().byte_range(), 21..51);
    assert_eq!(
        context.pieces(21..51).unwrap().pieces()[0].bytes(),
        b"method f {} {\n# audit\nreturn\n}"
    );
}

#[cfg(feature = "python")]
#[test]
fn mapping_coordinates_preserve_complete_tokens_and_disjoint_origins() {
    let source = b"\"a\\x40b\"\n\"@au\" # ordinary\n\"dit\"";
    let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let maps = [
        Mapping {
            output: TextRange::new("a@b", 0..1).unwrap(),
            origin: Origin::Copied(context.pieces(1..2).unwrap()),
        },
        Mapping {
            output: TextRange::new("a@b", 1..2).unwrap(),
            origin: Origin::Decoded(context.pieces(2..6).unwrap()),
        },
        Mapping {
            output: TextRange::new("a@b", 2..3).unwrap(),
            origin: Origin::Copied(context.pieces(6..7).unwrap()),
        },
    ];
    let Origin::Decoded(token) = &maps[1].origin else {
        panic!("decoded token");
    };
    assert_eq!(token.pieces()[0].range(), 2..6);
    assert_eq!(token.pieces()[0].bytes(), b"\\x40");
    assert_eq!(maps[1].output.range(), 1..2);
    let concat = [
        context.pieces(10..13).unwrap(),
        context.pieces(27..30).unwrap(),
    ];
    assert_eq!(concat[0].pieces()[0].bytes(), b"@au");
    assert_eq!(concat[1].pieces()[0].bytes(), b"dit");
    let synthetic = Mapping {
        output: TextRange::new("a\nb", 1..2).unwrap(),
        origin: Origin::Synthetic,
    };
    assert_eq!(synthetic.output.range(), 1..2);
    assert_eq!(synthetic.origin, Origin::Synthetic);
}

#[cfg(feature = "rust")]
#[test]
fn independent_stores_have_distinct_handles_and_explicit_role_order() {
    let source = b"///a\nfn f() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let roles = [
        DocumentRole::Primary,
        DocumentRole::Additional,
        DocumentRole::AttributeBinding,
        DocumentRole::Sphinx,
        DocumentRole::LeadingComment,
        DocumentRole::BodyComment,
        DocumentRole::FilePackage,
        DocumentRole::Virtual,
        DocumentRole::Inferred,
    ];
    let build = || {
        let mut facts = Facts::default();
        facts.fragments.push(Fragment {
            source: context.pieces(0..4).unwrap(),
            node: None,
            form: FragmentForm::Comment,
        });
        for role in roles {
            facts.documents.push(Document {
                fragments: vec![FragmentId(0)],
                composition: Composition::Single,
                role,
                content: Content::MetadataOnly,
            });
        }
        facts
    };
    let first = build();
    let second = build();
    assert_ne!(
        first.fragments().next().unwrap(),
        second.fragments().next().unwrap()
    );
    assert_ne!(
        first.documents().next().unwrap(),
        second.documents().next().unwrap()
    );
    let keys: Vec<_> = first
        .documents()
        .map(|doc| first.document_order(doc).unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            (0, 4, 0, 0),
            (0, 4, 1, 1),
            (0, 4, 2, 2),
            (0, 4, 3, 3),
            (0, 4, 4, 4),
            (0, 4, 5, 5),
            (0, 4, 6, 6),
            (0, 4, 7, 7),
            (0, 4, 8, 8)
        ]
    );
}
