//! Runnable copies of each Rust snippet in `library/documentation.md`.

#[cfg(feature = "rust")]
#[test]
fn collect_and_read() {
    use big_code_analysis::{Ast, LANG, Source};
    use big_code_analysis_ast::documentation::{DocumentationContext, DocumentationOptions};

    let ast =
        Ast::parse(Source::new(LANG::Rust, b"/// audit\nfn f() {}")).expect("rust feature enabled");
    let index = ast
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .expect("Rust adapter available");
    assert_eq!(index.documents().len(), 1);
    let document = index.documents().next().expect("one document");
    let fragment = index
        .facts()
        .document_fragment(document, 0)
        .expect("first fragment");
    assert_eq!(fragment.get().source().pieces()[0].bytes(), b"/// audit");
    let complete = index
        .complete_text(document)
        .expect("same index")
        .expect("complete text");
    assert_eq!(complete.text(), "audit");
}

#[cfg(feature = "rust")]
#[test]
fn owners_and_origins() {
    use big_code_analysis::{Ast, LANG, Source};
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, OriginView, Owner, Resolution,
    };

    let ast = Ast::parse(Source::new(
        LANG::Rust,
        b"/// @audit\n#[doc = \"detail\"]\nfn f() {}",
    ))
    .expect("rust feature enabled");
    let index = ast
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .expect("Rust adapter available");
    let document = index.documents().next().expect("one document");
    assert_eq!(document.get().fragment_count(), 2);
    let complete = index
        .complete_text(document)
        .expect("same index")
        .expect("complete text");
    assert_eq!(complete.text(), "@audit\ndetail");
    let annotation = complete.text().find("@audit").expect("annotation");
    let origin: Vec<_> = complete
        .origins(annotation..annotation + "@audit".len())
        .expect("valid UTF-8 byte range")
        .collect();
    assert_eq!(origin.len(), 1);
    let OriginView::Copied(piece) = &origin[0].origin else {
        panic!("copied annotation")
    };
    assert_eq!(piece.bytes(), b"@audit");
    assert_eq!(piece.range(), 4..10);
    let all: Vec<_> = complete
        .origins(0..complete.text().len())
        .expect("valid range")
        .collect();
    assert!(all.len() > 1);
    assert!(
        all.iter()
            .any(|portion| matches!(portion.origin, OriginView::Synthetic))
    );
    let association = index.associations().next().expect("one association");
    assert_eq!(association.get().resolution, Resolution::Resolved);
    assert_eq!(association.get().owner_count(), 1);
    assert_eq!(
        index
            .facts()
            .associated_document(association)
            .expect("same index"),
        document
    );
    let owner = index
        .facts()
        .associated_owner(association, 0)
        .expect("resolved owner");
    let Owner::Node(node) = owner.get() else {
        panic!("function owner")
    };
    assert_eq!(node.node().kind(), "function_item");
    let matches = index
        .node_associations(node.node())
        .expect("canonical node");
    assert_eq!(matches.associations().count(), 1);
}

#[cfg(feature = "rust")]
#[test]
fn empty_and_errors() {
    use big_code_analysis::{Ast, LANG, Source};
    use big_code_analysis_ast::documentation::{
        CollectionError, DocumentationContext, DocumentationOptions, Profile,
    };

    let context = DocumentationContext::default();
    let empty = Ast::parse(Source::new(LANG::Rust, b"")).expect("rust feature enabled");
    let index = empty
        .documentation(&context, &DocumentationOptions::default())
        .expect("empty but complete");
    assert_eq!(index.documents().len(), 0);
    let ast =
        Ast::parse(Source::new(LANG::Rust, b"/// audit\nfn f() {}")).expect("rust feature enabled");
    let wrong = DocumentationOptions {
        profile: Profile::Javadoc,
        ..DocumentationOptions::default()
    };
    assert_eq!(
        ast.documentation(&context, &wrong).err(),
        Some(CollectionError::UnsupportedProfile {
            language: LANG::Rust,
            profile: Profile::Javadoc
        })
    );
}

#[cfg(feature = "rust")]
#[test]
fn present_empty_and_unavailable() {
    use big_code_analysis::{Ast, LANG, Source};
    use big_code_analysis_ast::documentation::{
        Content, DocumentationContext, DocumentationOptions,
    };

    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let empty = Ast::parse(Source::new(LANG::Rust, b"#[doc = \"\"] fn f() {}"))
        .expect("rust feature enabled");
    let index = empty
        .documentation(&context, &options)
        .expect("Rust adapter available");
    assert_eq!(index.documents().len(), 1);
    let document = index.documents().next().expect("present document");
    assert_eq!(
        index
            .complete_text(document)
            .expect("same index")
            .expect("complete")
            .text(),
        ""
    );

    let computed = Ast::parse(Source::new(LANG::Rust, br#"#[doc = b"\xff"] fn f() {}"#))
        .expect("rust feature enabled");
    let index = computed
        .documentation(&context, &options)
        .expect("Rust adapter available");
    let document = index.documents().next().expect("present document");
    assert!(matches!(document.get().content(), Content::Unavailable(_)));
    assert!(index.complete_text(document).expect("same index").is_none());
    let fragment = index
        .facts()
        .document_fragment(document, 0)
        .expect("raw fragment");
    assert!(
        fragment.get().source().pieces()[0]
            .bytes()
            .starts_with(b"#[doc =")
    );
}

// test-lang-gates: hand-written(rust) — from_path selects Rust from the .rs suffix
#[cfg(feature = "rust")]
#[test]
fn normalized_path_coordinates() {
    use big_code_analysis::Ast;
    use big_code_analysis_ast::documentation::{DocumentationContext, DocumentationOptions};

    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("example.rs");
    std::fs::write(&path, b"/// a\r\n/// b\r\nfn f() {}\r\n").expect("write source");
    let ast = Ast::from_path(&path).expect("read Rust source");
    assert_eq!(ast.source(), b"/// a\n/// b\nfn f() {}\n");
    let index = ast
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .expect("Rust adapter available");
    assert_eq!(
        index.context().logical_filename.as_deref(),
        Some("example.rs")
    );
    let document = index.documents().next().expect("one document");
    let second = index
        .facts()
        .document_fragment(document, 1)
        .expect("second fragment");
    assert_eq!(second.get().source().pieces()[0].range(), 6..11);
    assert_eq!(
        index
            .complete_text(document)
            .expect("same index")
            .expect("complete")
            .text(),
        "a\nb"
    );
}
