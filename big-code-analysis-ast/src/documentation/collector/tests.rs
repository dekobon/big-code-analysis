use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "typescript"
))]
fn assert_positive(
    language: LANG,
    source: &[u8],
    fragment: std::ops::Range<usize>,
    owner: std::ops::Range<usize>,
    text: &str,
) {
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.root_node().has_error(),
        "{language:?}: {:?}",
        parser.root_node()
    );
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    for index in [
        parser.documentation(&context, &options).unwrap(),
        collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ] {
        let documents: Vec<_> = index.documents().collect();
        assert_eq!(documents.len(), 1, "{language:?}");
        let document = documents[0];
        assert_eq!(document.get().fragment_count(), 1);
        let raw = index.facts().document_fragment(document, 0).unwrap();
        assert_eq!(raw.get().source().pieces().len(), 1);
        assert_eq!(raw.get().source().pieces()[0].range(), fragment);
        let complete = index.complete_text(document).unwrap().unwrap();
        assert_eq!(complete.text(), text, "{language:?}");
        let associations: Vec<_> = index.associations().collect();
        assert_eq!(associations.len(), 1, "{language:?}");
        assert_eq!(associations[0].get().resolution, Resolution::Resolved);
        assert_eq!(associations[0].get().publication, Publication::Unknown);
        let target = index.facts().associated_owner(associations[0], 0).unwrap();
        let Owner::Node(node) = target.get() else {
            panic!("expected node owner")
        };
        assert_eq!(node.node().byte_range(), owner, "{language:?}");
        let query = index.node_associations(node.node()).unwrap();
        assert_eq!(query.associations().count(), 1);
        let origins: Vec<_> = complete.origins(0..text.len()).unwrap().collect();
        assert_eq!(origins.len(), 1);
        let OriginView::Copied(piece) = &origins[0].origin else {
            panic!("copied origin")
        };
        assert_eq!(piece.bytes(), text.as_bytes());
        let offset = match language {
            LANG::Php => 4,
            LANG::Ruby => 2,
            _ => 3,
        };
        assert_eq!(
            piece.range(),
            fragment.start + offset..fragment.start + offset + text.len()
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_production_positive() {
    assert_positive(LANG::Rust, b"///audit\nfn f() {}", 0..8, 9..18, "audit");
}
#[cfg(feature = "java")]
#[test]
fn java_production_positive() {
    assert_positive(LANG::Java, b"///audit\nclass C {}", 0..8, 9..19, "audit");
}
#[cfg(feature = "kotlin")]
#[test]
fn kotlin_production_positive() {
    assert_positive(
        LANG::Kotlin,
        b"/**audit*/\nfun f() {}",
        0..10,
        11..21,
        "audit",
    );
}
#[cfg(feature = "groovy")]
#[test]
fn groovy_production_positive() {
    assert_positive(
        LANG::Groovy,
        b"/**audit*/\nclass C {}",
        0..10,
        11..21,
        "audit",
    );
}
#[cfg(feature = "csharp")]
#[test]
fn csharp_production_positive() {
    assert_positive(
        LANG::Csharp,
        b"///<summary>audit</summary>\nclass C {}",
        0..27,
        28..38,
        "<summary>audit</summary>",
    );
}
#[cfg(feature = "php")]
#[test]
fn php_production_positive() {
    assert_positive(
        LANG::Php,
        b"<?php\n/** audit */\nfunction f() {}",
        6..18,
        19..34,
        "audit",
    );
}
#[cfg(feature = "javascript")]
#[test]
fn javascript_production_positive() {
    assert_positive(
        LANG::Javascript,
        b"/**audit*/\nfunction f() {}",
        0..10,
        11..26,
        "audit",
    );
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_production_positive() {
    assert_positive(
        LANG::Mozjs,
        b"/**audit*/\nfunction f() {}",
        0..10,
        11..26,
        "audit",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_production_positive() {
    assert_positive(
        LANG::Typescript,
        b"/**audit*/\nfunction f(): void {}",
        0..10,
        11..32,
        "audit",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_production_positive() {
    assert_positive(
        LANG::Tsx,
        b"/**audit*/\nfunction f() { return <div/>; }",
        0..10,
        11..42,
        "audit",
    );
}
#[cfg(feature = "c")]
#[test]
fn c_production_positive() {
    assert_positive(
        LANG::C,
        b"/**audit*/\nvoid f(void) {}",
        0..10,
        11..26,
        "audit",
    );
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_production_positive() {
    assert_positive(
        LANG::Cpp,
        b"/**audit*/\nvoid f() {}",
        0..10,
        11..22,
        "audit",
    );
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_production_positive() {
    assert_positive(
        LANG::Mozcpp,
        b"/**audit*/\nvoid f() {}",
        0..10,
        11..22,
        "audit",
    );
}
#[cfg(feature = "objc")]
#[test]
fn objc_production_positive() {
    assert_positive(
        LANG::Objc,
        b"/**audit*/\nvoid f(void) {}",
        0..10,
        11..26,
        "audit",
    );
}
#[cfg(feature = "go")]
#[test]
fn go_production_positive() {
    assert_positive(
        LANG::Go,
        b"package p\n// audit\nfunc F() {}",
        10..18,
        19..30,
        "audit",
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_production_positive() {
    assert_positive(LANG::Ruby, b"# audit\ndef f\nend", 0..7, 8..17, "audit");
}
#[cfg(feature = "lua")]
#[test]
fn lua_production_positive() {
    assert_positive(
        LANG::Lua,
        b"module(\"m\")\n---audit\nfunction f() end",
        12..20,
        21..37,
        "audit",
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_joint_smoke_pins_exact_composition_and_maps() {
    let parser =
        AnyParser::parse(LANG::Rust, b"///a\n///b\nfn f() {}".to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let document = index.documents().next().unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(document.get().fragment_count(), 2);
    assert_eq!(
        index
            .facts()
            .document_fragment(document, 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .range(),
        0..4
    );
    assert_eq!(
        index
            .facts()
            .document_fragment(document, 1)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .range(),
        5..9
    );
    let text = index.complete_text(document).unwrap().unwrap();
    assert_eq!(text.text(), "a\nb");
    let portions: Vec<_> = text.origins(0..3).unwrap().collect();
    assert_eq!(portions.len(), 3);
    let OriginView::Copied(a) = &portions[0].origin else {
        panic!("copied a")
    };
    assert_eq!(a.range(), 3..4);
    assert_eq!(portions[1].origin, OriginView::Synthetic);
    assert_eq!(portions[1].output, 1..2);
    let OriginView::Copied(b) = &portions[2].origin else {
        panic!("copied b")
    };
    assert_eq!(b.range(), 8..9);
}

#[cfg(feature = "rust")]
#[test]
fn rust_pending_attributes_never_report_complete_partial_text() {
    for (source, forms) in [
        (
            "#[doc = concat!(\"audit\")]\nfn f() {}",
            vec![FragmentForm::Attribute],
        ),
        (
            "/// first\n#[doc(alias = \"second\")]\nfn f() {}",
            vec![FragmentForm::Comment, FragmentForm::Attribute],
        ),
        (
            "#[doc = include_str!(\"first\")]\n/// second\nfn f() {}",
            vec![FragmentForm::Attribute, FragmentForm::Comment],
        ),
        ("#[doc(hidden)]\nfn f() {}", vec![FragmentForm::Attribute]),
        (
            "#[cfg_attr(feature = \"x\", doc = \"audit\")]\nfn f() {}",
            vec![FragmentForm::Attribute],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1, "{source}");
        let document = index.documents().next().unwrap();
        assert!(matches!(document.get().content(), Content::PendingAdapter));
        assert!(index.complete_text(document).unwrap().is_none());
        assert_eq!(document.get().role(), DocumentRole::Primary);
        assert_eq!(document.get().fragment_count(), forms.len());
        for (position, form) in forms.into_iter().enumerate() {
            assert_eq!(
                index
                    .facts()
                    .document_fragment(document, position)
                    .unwrap()
                    .get()
                    .form(),
                form
            );
        }
        let owner = parser
            .root_node()
            .children()
            .find(|node| node.kind() == "function_item")
            .unwrap();
        let query = index.node_associations(owner.as_tree_sitter()).unwrap();
        assert_eq!(query.associations().count(), 1);
        assert!(
            query
                .assessments()
                .any(|assessment| assessment.get().axis == AssessmentAxis::Composition)
        );
    }
    for source in [
        "#[document]\nfn f() {}",
        "#[test]\nfn f() {}",
        "#[rustfmt::skip]\nfn f() {}",
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 0, "{source}");
    }
}

#[cfg(feature = "rust")]
#[test]
fn canonical_queries_reject_foreign_nodes_handles_and_edited_values() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<DocumentationIndex<'_>>();
    let source = b"fn f() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let foreign = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let index = parser.documentation(&context, &options).unwrap();
    let second = parser.documentation(&context, &options).unwrap();
    let node = parser
        .root_node()
        .children()
        .next()
        .unwrap()
        .as_tree_sitter();
    assert_eq!(
        index.node_associations(node).unwrap().associations().len(),
        0
    );
    assert!(matches!(
        index.node_associations(
            foreign
                .root_node()
                .children()
                .next()
                .unwrap()
                .as_tree_sitter()
        ),
        Err(OwnerError::Source(SourceError::ForeignTree))
    ));
    let mut edited = node;
    edited.edit(&tree_sitter::InputEdit {
        start_byte: 0,
        old_end_byte: 0,
        new_end_byte: 1,
        start_position: tree_sitter::Point::new(0, 0),
        old_end_position: tree_sitter::Point::new(0, 0),
        new_end_position: tree_sitter::Point::new(0, 1),
    });
    assert!(matches!(
        index.node_associations(edited),
        Err(OwnerError::Source(SourceError::NoncanonicalNode))
    ));
    assert!(matches!(
        index.owner_associations(second.facts().owners().next().unwrap()),
        Err(OwnerError::Fact(FactError::ForeignIndex))
    ));
    assert!(matches!(
        index.node_associations(node.child(0).unwrap()),
        Err(OwnerError::UnsupportedOwner)
    ));
}

#[cfg(feature = "rust")]
#[test]
fn collector_structural_counters_and_retained_reads_price_real_work() {
    use super::{child_batches, owner_projections, target_classifications, visits};
    let parser = AnyParser::parse(
        LANG::Rust,
        b"///audit\nfn f() {}\nfn g() {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let count = parser.root_node().preorder().count();
    let before = (
        visits::observed(),
        child_batches::observed(),
        target_classifications::observed(),
        owner_projections::observed(),
        crate::documentation::text::renders::observed(),
    );
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(visits::observed() - before.0, count);
    assert_eq!(child_batches::observed() - before.1, count);
    assert_eq!(target_classifications::observed() - before.2, count - 1);
    assert_eq!(owner_projections::observed() - before.3, 3);
    assert_eq!(
        crate::documentation::text::renders::observed() - before.4,
        1
    );
    let document = index.documents().next().unwrap();
    let owner = parser.root_node().children().last().unwrap();
    let before = (
        crate::documentation::canonical::identity_probes::observed(),
        crate::documentation::text::renders::observed(),
        crate::documentation::text::copy_chunks::observed(),
    );
    for _ in 0..128 {
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "audit"
        );
        assert_eq!(
            index
                .node_associations(owner.as_tree_sitter())
                .unwrap()
                .associations()
                .len(),
            0
        );
    }
    assert_eq!(
        crate::documentation::canonical::identity_probes::observed() - before.0,
        128
    );
    assert_eq!(
        crate::documentation::text::renders::observed() - before.1,
        0
    );
    assert_eq!(
        crate::documentation::text::copy_chunks::observed() - before.2,
        0
    );
}

#[cfg(feature = "c")]
#[test]
fn c_declaration_fanout_and_trailing_projection_share_retained_text() {
    for (source, names) in [
        ("/**audit*/\nint a, b;", vec!["a", "b"]),
        ("struct S { int x; ///<audit\n};", vec!["x"]),
    ] {
        let parser = AnyParser::parse(LANG::C, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "audit"
        );
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().owner_count(), names.len());
        for (position, name) in names.into_iter().enumerate() {
            let owner = index
                .facts()
                .associated_owner(association, position)
                .unwrap();
            let Owner::Slot(slot) = owner.get() else {
                panic!("expected field slot")
            };
            assert_eq!(slot.source.pieces()[0].bytes(), name.as_bytes());
            assert_eq!(slot.ordinal, position);
            assert_eq!(
                index
                    .owner_associations(owner)
                    .unwrap()
                    .associations()
                    .len(),
                1
            );
        }
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_wrappers_resolve_to_explicit_declarations() {
    for source in [
        "/**audit*/\nexport function f() {}",
        "/**audit*/\ndeclare function f(): void;",
    ] {
        let parser =
            AnyParser::parse(LANG::Typescript, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("explicit declaration")
        };
        assert!(matches!(
            node.node().kind(),
            "function_declaration" | "function_signature"
        ));
        assert_ne!(node.node(), association.get().binding.unwrap().node());
        assert_eq!(
            index
                .node_associations(node.node())
                .unwrap()
                .associations()
                .len(),
            1
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn raw_input_checks_and_cancellation_have_distinct_outcomes() {
    let parser = AnyParser::parse(LANG::Rust, b"fn f() {}".to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    assert!(matches!(
        collect(LANG::Rust, parser.ts_tree(), b"short", &context, &options),
        Err(CollectionError::SourceBounds)
    ));
    let incompatible = DocumentationOptions {
        profile: Profile::Python,
        ..options
    };
    assert!(matches!(
        parser.documentation(&context, &incompatible),
        Err(CollectionError::UnsupportedProfile {
            language: LANG::Rust,
            profile: Profile::Python
        })
    ));
    let cancel = crate::cancel::ParseCancel::new();
    cancel.cancel();
    assert!(matches!(
        cancel.run(|| parser.documentation(&context, &incompatible)),
        Err(CollectionError::Cancelled)
    ));
    assert_eq!(
        parser
            .documentation(&context, &options)
            .unwrap()
            .documents()
            .len(),
        0
    );
    let empty = AnyParser::parse(LANG::Rust, Vec::new(), None, None).unwrap();
    assert_eq!(
        empty
            .documentation(&context, &options)
            .unwrap()
            .documents()
            .len(),
        0
    );
    let source = b"fn f() {}";
    let mut raw = tree_sitter::Parser::new();
    raw.set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    raw.set_included_ranges(&[tree_sitter::Range {
        start_byte: 0,
        end_byte: 1000,
        start_point: tree_sitter::Point::new(0, 0),
        end_point: tree_sitter::Point::new(0, 1000),
    }])
    .unwrap();
    let tree = raw.parse(source, None).unwrap();
    assert_eq!(
        collect(LANG::Rust, &tree, source, &context, &options)
            .unwrap()
            .documents()
            .len(),
        0
    );
}

// test-lang-gates: hand-written(rust) — collect resolves the requested Rust grammar before comparing the Java tree.
#[cfg(all(feature = "rust", feature = "java"))]
#[test]
fn mismatched_raw_grammar_is_rejected_before_predicates() {
    let parser = AnyParser::parse(LANG::Java, b"class C {}".to_vec(), None, None).unwrap();
    assert!(matches!(
        collect(
            LANG::Rust,
            parser.ts_tree(),
            parser.code(),
            &DocumentationContext::default(),
            &DocumentationOptions::default()
        ),
        Err(CollectionError::GrammarMismatch)
    ));
}

#[cfg(feature = "ruby")]
#[test]
fn unavailable_adapter_is_not_an_empty_success() {
    // A documented source under a profile whose adapter does not exist is rejected.
    // RDoc for Ruby is such a profile whichever language adapters are switched on.
    let parser =
        AnyParser::parse(LANG::Ruby, b"# audit\ndef f\nend\n".to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    assert_eq!(
        parser
            .documentation(&context, &DocumentationOptions::default())
            .unwrap()
            .documents()
            .len(),
        1
    );
    let options = DocumentationOptions {
        profile: Profile::Rdoc,
        ..DocumentationOptions::default()
    };
    assert!(matches!(
        parser.documentation(&context, &options),
        Err(CollectionError::AdapterUnavailable {
            language: LANG::Ruby,
            profile: Profile::Rdoc
        })
    ));
}

#[cfg(feature = "c-family-helpers")]
#[test]
fn helper_capability_is_explicit_and_owner_lookup_is_unsupported() {
    for (language, source) in [
        (LANG::Ccomment, b"/**audit*/".as_slice()),
        (LANG::Preproc, b"#define X 1\n".as_slice()),
    ] {
        let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.capability(), Capability::NoDeclarationModel);
        assert_eq!(index.documents().len(), 0);
        assert_eq!(index.associations().len(), 0);
        assert!(matches!(
            index.node_associations(parser.ts_tree().root_node()),
            Err(OwnerError::UnsupportedOwner)
        ));
    }
}

#[cfg(feature = "cpp")]
#[test]
fn doxygen_static_names_resolve_unique_missing_and_ambiguous_targets() {
    for (source, resolution, owners) in [
        (
            "/**@fn void f()\n * audit*/\nvoid f();",
            Resolution::Resolved,
            1,
        ),
        (
            "/**@fn void f()\n * audit*/\nvoid f() {}",
            Resolution::Resolved,
            1,
        ),
        (
            "void f() {}\n/**@fn void f()\n * audit*/",
            Resolution::Resolved,
            1,
        ),
        (
            "/**@fn void missing()\n * audit*/\nvoid f();",
            Resolution::Unresolved(UnresolvedReason::MissingName),
            0,
        ),
        (
            "/**@fn f\n * audit*/\nvoid f(int);\nvoid f(double);",
            Resolution::Unresolved(UnresolvedReason::Ambiguous),
            2,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1, "{source}");
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, resolution, "{source}");
        assert_eq!(association.get().owner_count(), owners);
        assert!(
            index
                .complete_text(index.documents().next().unwrap())
                .unwrap()
                .unwrap()
                .text()
                .ends_with("\naudit")
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn deep_missing_collection_and_teardown_fit_a_small_stack() {
    let source = format!("fn f() {{ {}0{}; }}", "(".repeat(1600), ")".repeat(1599));
    let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
    let missing = parser
        .root_node()
        .preorder()
        .find(crate::Node::is_missing)
        .unwrap();
    let before = crate::node::node_resolved_parent_lookups::observed();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        index.node_associations(missing.as_tree_sitter()),
        Err(OwnerError::UnsupportedOwner)
    ));
    if cfg!(chain_audit) {
        assert!(crate::node::node_resolved_parent_lookups::observed() > before);
    } else {
        assert_eq!(
            crate::node::node_resolved_parent_lookups::observed() - before,
            0
        );
    }
    drop(index);
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            if !cfg!(chain_audit) {
                let index = parser
                    .documentation(
                        &DocumentationContext::default(),
                        &DocumentationOptions::default(),
                    )
                    .unwrap();
                assert_eq!(index.documents().len(), 0);
                drop(index);
            }
            drop(parser);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "rust")]
#[test]
fn local_inner_docs_use_enclosing_items_and_macro_docs_stay_unresolved() {
    for (source, kind, role) in [
        (
            "//!audit\nfn f() {}",
            "source_file",
            DocumentRole::FilePackage,
        ),
        ("mod m { //!audit\n}", "mod_item", DocumentRole::FilePackage),
        (
            "fn f() { //!audit\n}",
            "function_item",
            DocumentRole::Primary,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(document.get().role(), role);
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "audit"
        );
        let association = index.associations().next().unwrap();
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let (Owner::Node(node) | Owner::File(node)) = owner.get() else {
            panic!("enclosing item or file")
        };
        assert_eq!(node.node().kind(), kind);
    }
    let parser = AnyParser::parse(
        LANG::Rust,
        b"m! { ///audit\nfn f() {} }".to_vec(),
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
    assert_eq!(index.documents().len(), 1);
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().resolution,
        Resolution::Unresolved(UnresolvedReason::Expansion)
    );
    assert_eq!(association.get().owner_count(), 0);
    assert_eq!(
        index
            .complete_text(index.documents().next().unwrap())
            .unwrap()
            .unwrap()
            .text(),
        "audit"
    );
}

#[cfg(feature = "rust")]
#[test]
fn deterministic_collection_checkpoints_abort_without_successful_partial_indices() {
    let source = format!(
        "///{}\nfn f() {{}}\n{}",
        "a".repeat(40_000),
        "fn g() {}\n".repeat(300)
    );
    let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations().polls;
    let completed = parser.documentation(&context, &options).unwrap();
    let checkpoints = observations().polls - before;
    assert_eq!(completed.documents().len(), 1);
    assert!(checkpoints > 10);
    for stop in [1, 2, 3, checkpoints / 2, checkpoints - 1, checkpoints] {
        assert!(
            matches!(
                crate::documentation::budget::cancel_after(stop, || parser
                    .documentation(&context, &options)),
                Err(CollectionError::Cancelled)
            ),
            "checkpoint {stop}"
        );
        assert_eq!(
            parser
                .documentation(&context, &options)
                .unwrap()
                .documents()
                .len(),
            1
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_group_and_spec_have_separate_documents_and_owners() {
    let source = b"package p\n// group\nvar (\n// member\nX int\n)";
    let parser = AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let documents: Vec<_> = index.documents().collect();
    assert_eq!(documents.len(), 2);
    assert_eq!(
        index.complete_text(documents[0]).unwrap().unwrap().text(),
        "group"
    );
    assert_eq!(
        index.complete_text(documents[1]).unwrap().unwrap().text(),
        "member"
    );
    let associations: Vec<_> = index.associations().collect();
    assert_eq!(associations.len(), 2);
    for (association, kind) in associations
        .into_iter()
        .zip(["var_declaration", "var_spec"])
    {
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("group or spec")
        };
        assert_eq!(node.node().kind(), kind);
        assert_eq!(
            index
                .node_associations(node.node())
                .unwrap()
                .associations()
                .len(),
            1
        );
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_modifier_and_body_wrappers_project_the_semantic_method() {
    for source in [
        "# audit\ndef f; end if true",
        "class C\n# audit\ndef f\nend\nend",
    ] {
        let parser = AnyParser::parse(LANG::Ruby, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1, "{source}");
        let association = index.associations().next().unwrap();
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("method")
        };
        assert_eq!(node.node().kind(), "method");
        assert_eq!(
            index
                .node_associations(node.node())
                .unwrap()
                .associations()
                .len(),
            1
        );
    }
}

#[cfg(feature = "java")]
#[test]
fn java_selected_empty_document_keeps_superseded_raw_evidence_only_on_request() {
    let parser = AnyParser::parse(
        LANG::Java,
        b"/** old */\n/**/\nclass C {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    for retain_unbound in [false, true] {
        let options = DocumentationOptions {
            retain_unbound,
            ..DocumentationOptions::default()
        };
        let index = parser
            .documentation(&DocumentationContext::default(), &options)
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(index.complete_text(document).unwrap().unwrap().text(), "");
        assert_eq!(index.evidence().len(), usize::from(retain_unbound));
        if retain_unbound {
            let evidence = index.evidence().next().unwrap();
            assert_eq!(evidence.get().disposition, CandidateDisposition::Superseded);
            assert_eq!(
                index
                    .evidence_fragment(evidence)
                    .unwrap()
                    .get()
                    .source()
                    .pieces()[0]
                    .bytes(),
                b"/** old */"
            );
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn document_handles_are_index_qualified_even_for_one_shared_parse() {
    let parser = AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let first = parser.documentation(&context, &options).unwrap();
    let second = parser.documentation(&context, &options).unwrap();
    let document = first.documents().next().unwrap();
    assert!(matches!(
        second.complete_text(document),
        Err(FactError::ForeignIndex)
    ));
    assert_eq!(
        first.complete_text(document).unwrap().unwrap().text(),
        "audit"
    );
}

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript"
))]
#[test]
fn named_profiles_distinguish_deferred_incompatible_and_available() {
    let defaults = [
        (LANG::Rust, Profile::Rust),
        (LANG::Python, Profile::Python),
        (LANG::Java, Profile::Javadoc),
        (LANG::Kotlin, Profile::Kdoc),
        (LANG::Groovy, Profile::Groovydoc),
        (LANG::Csharp, Profile::CsharpXml),
        (LANG::Php, Profile::BcaPhpSource),
        (LANG::Javascript, Profile::Jsdoc),
        (LANG::Mozjs, Profile::Jsdoc),
        (LANG::Typescript, Profile::BcaTypescriptSource),
        (LANG::Tsx, Profile::BcaTypescriptSource),
        (LANG::C, Profile::Doxygen),
        (LANG::Cpp, Profile::Doxygen),
        (LANG::Mozcpp, Profile::Doxygen),
        (LANG::Objc, Profile::Doxygen),
        (LANG::Go, Profile::GoDoc),
        (LANG::Ruby, Profile::Yard),
        (LANG::Lua, Profile::Ldoc),
        (LANG::Perl, Profile::Pod),
        (LANG::Elixir, Profile::Elixir),
        (LANG::Bash, Profile::Shdoc),
        (LANG::Tcl, Profile::Ruff),
        (LANG::Irules, Profile::Irules),
        (LANG::Ccomment, Profile::Helper),
        (LANG::Preproc, Profile::Helper),
    ];
    assert_eq!(defaults.len(), LANG::into_enum_iter().count());
    assert!(LANG::into_enum_iter().any(|language| language.is_enabled()));
    let context = DocumentationContext::default();
    for (language, default) in defaults {
        let parser = language
            .is_enabled()
            .then(|| AnyParser::parse(language, Vec::new(), None, None).unwrap());
        for requested in defaults.iter().map(|(_, profile)| *profile).chain([
            Profile::Default,
            Profile::Rdoc,
            Profile::PhpDocumentor,
        ]) {
            let profile = if requested == Profile::Default {
                default
            } else {
                requested
            };
            let alternate = (language == LANG::Ruby && profile == Profile::Rdoc)
                || (language == LANG::Php && profile == Profile::PhpDocumentor);
            let expected = if profile != default && !alternate {
                Err(CollectionError::UnsupportedProfile { language, profile })
            } else if alternate || [LANG::Bash, LANG::Irules].contains(&language) {
                Err(CollectionError::AdapterUnavailable { language, profile })
            } else {
                Ok(profile)
            };
            assert_eq!(requested.resolve(language), expected);
            if let Some(parser) = &parser {
                let options = DocumentationOptions {
                    profile: requested,
                    ..DocumentationOptions::default()
                };
                assert_eq!(
                    parser
                        .documentation(&context, &options)
                        .map(|index| index.profile()),
                    expected
                );
                assert_eq!(
                    collect(
                        language,
                        parser.ts_tree(),
                        parser.code(),
                        &context,
                        &options
                    )
                    .map(|index| index.profile()),
                    expected
                );
            }
        }
        if let Some(parser) = &parser {
            for disabled in LANG::into_enum_iter().filter(|language| !language.is_enabled()) {
                assert!(
                    matches!(collect(disabled, parser.ts_tree(), parser.code(), &context, &DocumentationOptions::default()), Err(CollectionError::LanguageDisabled(actual)) if actual == disabled)
                );
            }
        }
    }
}
