use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(feature = "rust")]
#[test]
#[allow(clippy::too_many_lines)]
fn rust_same_target_composition_and_roles_are_independent_of_placement() {
    for (source, kind, expected, fragments, role) in [
        (
            "/// outer\nmod m { //! inner\n}\n",
            "mod_item",
            Some("outer\ninner"),
            2,
            DocumentRole::FilePackage,
        ),
        (
            "/// outer\nfn f() { //! inner\n}\n",
            "function_item",
            Some("outer\ninner"),
            2,
            DocumentRole::Primary,
        ),
        (
            "/// outer\n#[doc = \"attribute\"]\nfn f() { //! inner\n}\n",
            "function_item",
            Some("outer\nattribute\ninner"),
            3,
            DocumentRole::Primary,
        ),
        (
            "/// outer\nmod m {}",
            "mod_item",
            Some("outer"),
            1,
            DocumentRole::FilePackage,
        ),
        (
            "mod m { //! inner\n}",
            "mod_item",
            Some("inner"),
            1,
            DocumentRole::FilePackage,
        ),
        (
            "//! first\n#![allow(unused)]\n//! second\n",
            "source_file",
            Some("first\nsecond"),
            2,
            DocumentRole::FilePackage,
        ),
        (
            "//! first\n#![doc = \"middle\"]\n//! second\n",
            "source_file",
            Some("first\nmiddle\nsecond"),
            3,
            DocumentRole::FilePackage,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        for retain_unbound in [false, true] {
            let context = DocumentationContext::default();
            let options = DocumentationOptions {
                retain_unbound,
                ..DocumentationOptions::default()
            };
            for index in [
                parser.documentation(&context, &options).unwrap(),
                collect(
                    LANG::Rust,
                    parser.ts_tree(),
                    parser.code(),
                    &context,
                    &options,
                )
                .unwrap(),
            ] {
                assert_eq!(index.documents().len(), 1, "{source}");
                let document = index.documents().next().unwrap();
                assert_eq!(document.get().fragment_count(), fragments);
                assert_eq!(document.get().role(), role);
                assert_eq!(
                    index
                        .complete_text(document)
                        .unwrap()
                        .map(CompleteText::text),
                    expected
                );
                if expected.is_none() {
                    assert!(matches!(document.get().content(), Content::PendingAdapter));
                }
                let association = index.associations().next().unwrap();
                let owner = index.facts().associated_owner(association, 0).unwrap();
                let (Owner::Node(node) | Owner::File(node)) = owner.get() else {
                    panic!("node owner")
                };
                assert_eq!(node.node().kind(), kind);
                if source == "/// outer\nmod m { //! inner\n}\n" {
                    assert_eq!(node.node().byte_range(), 10..29);
                    let text = index.complete_text(document).unwrap().unwrap();
                    let origins: Vec<_> = text.origins(0..11).unwrap().collect();
                    assert_eq!(origins.len(), 3);
                    let OriginView::Copied(first) = &origins[0].origin else {
                        panic!("copy")
                    };
                    let OriginView::Copied(last) = &origins[2].origin else {
                        panic!("copy")
                    };
                    assert_eq!(first.range(), 4..9);
                    assert_eq!(last.range(), 22..27);
                    assert_eq!(origins[1].origin, OriginView::Synthetic);
                }
            }
        }
    }
}

#[cfg(feature = "ruby")]
fn pending_first(language: LANG, source: &str) {
    let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 0);
    let doc = index.documents().next().unwrap();
    assert_eq!(index.complete_text(doc).unwrap().unwrap().text(), "audit");
    assert!(
        index
            .assessments()
            .any(|a| index.assessed_document(a).unwrap() == Some(doc))
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_pending_first_statement_never_redirects_to_later_method() {
    pending_first(LANG::Ruby, "class C\n# audit\nX = 1\ndef later; end\nend");
}
#[cfg(feature = "lua")]
#[test]
fn lua_first_statement_owner_never_redirects_to_later_function() {
    let parser = AnyParser::parse(
        LANG::Lua,
        b"module(\"m\")\ndo\n---audit\nvalue = 1\nfunction later() end\nend".to_vec(),
        None,
        None,
    )
    .unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    for index in [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Lua,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ] {
        assert_eq!(index.documents().len(), 1);
        let [association] = index.associations().collect::<Vec<_>>()[..] else {
            panic!("one association")
        };
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("node owner")
        };
        assert_eq!(node.node().kind(), "assignment_statement");
        assert_eq!(node.node().byte_range(), 24..33);
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_signature_context_exposes_document_linked_owner_assessment() {
    for (source, kind) in [
        (
            "public /**audit*/ inline fun f() {}",
            "function_declaration",
        ),
        (
            "class C private /**audit*/ constructor()",
            "class_declaration",
        ),
        ("public /**audit*/ fun f() {}", "function_declaration"),
        ("class C /**audit*/ constructor()", "class_declaration"),
    ] {
        let parser =
            AnyParser::parse(LANG::Kotlin, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let document = index.documents().next().unwrap();
        let node = parser
            .root_node()
            .preorder()
            .find(|n| n.kind() == kind)
            .unwrap();
        assert_eq!(index.associations().len(), 0);
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "audit"
        );
        assert!(
            index
                .node_associations(node.as_tree_sitter())
                .unwrap()
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Owner
                    && index.assessed_document(a).unwrap() == Some(document)),
            "{source}"
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_conditional_docs_remain_pending_only_in_affected_scope() {
    for doc in ["///audit", "/**audit*/"] {
        let source = format!(
            "{doc}\n#if FLAG\nclass C {{}}\n#elif OTHER\nclass D {{}}\n#else\nclass E {{}}\n#endif\nclass Later {{}}"
        );
        let parser = AnyParser::parse(LANG::Csharp, source.into_bytes(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 0);
        let doc = index.documents().next().unwrap();
        assert_eq!(index.complete_text(doc).unwrap().unwrap().text(), "audit");
        let classes: Vec<_> = parser
            .root_node()
            .preorder()
            .filter(|n| n.kind() == "class_declaration")
            .collect();
        assert_eq!(classes.len(), 4);
        for class in &classes[..3] {
            assert!(
                index
                    .node_associations(class.as_tree_sitter())
                    .unwrap()
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Selection
                        && index.assessed_document(a).unwrap() == Some(doc))
            );
        }
        assert_eq!(
            index
                .node_associations(classes[3].as_tree_sitter())
                .unwrap()
                .assessments()
                .count(),
            0
        );
    }
}

#[cfg(feature = "c-family-helpers")]
#[test]
fn helpers_validate_all_canonical_nodes_before_unsupported_owner() {
    for (language, source) in [
        (LANG::Ccomment, b"/**audit*/".as_slice()),
        (LANG::Preproc, b"#define X 1\n".as_slice()),
        (LANG::Ccomment, b"".as_slice()),
    ] {
        let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.capability(), Capability::NoDeclarationModel);
        assert_eq!(index.facts().owners().len(), 0);
        for node in parser.root_node().preorder() {
            assert!(matches!(
                index.node_associations(node.as_tree_sitter()),
                Err(OwnerError::UnsupportedOwner)
            ));
            assert!(matches!(
                index.node_owners(node.as_tree_sitter()),
                Err(OwnerError::UnsupportedOwner)
            ));
            assert!(matches!(
                index.node_projection(node.as_tree_sitter()),
                Err(OwnerError::UnsupportedOwner)
            ));
        }
        let foreign = parser.ts_tree().clone();
        assert!(matches!(
            index.node_projection(foreign.root_node()),
            Err(OwnerError::Source(SourceError::ForeignTree))
        ));
        let mut edited = parser.ts_tree().root_node();
        edited.edit(&tree_sitter::InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 1,
            start_position: tree_sitter::Point::new(0, 0),
            old_end_position: tree_sitter::Point::new(0, 0),
            new_end_position: tree_sitter::Point::new(0, 1),
        });
        assert!(matches!(
            index.node_projection(edited),
            Err(OwnerError::Source(SourceError::NoncanonicalNode))
        ));
        assert!(matches!(
            index.node_owners(edited),
            Err(OwnerError::Source(SourceError::NoncanonicalNode))
        ));
        assert!(matches!(
            index.node_associations(edited),
            Err(OwnerError::Source(SourceError::NoncanonicalNode))
        ));
    }
}

#[cfg(feature = "c-family-helpers")]
#[test]
fn helper_canonical_walk_cancels_and_tears_down_on_small_stack() {
    use crate::documentation::budget::cancel_after;
    let parser =
        AnyParser::parse(LANG::Preproc, b"#define X 1\n".repeat(2048), None, None).unwrap();
    assert!(matches!(
        cancel_after(4, || parser.documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default()
        )),
        Err(CollectionError::Cancelled)
    ));
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            assert_eq!(index.documents().len(), 0);
            assert!(matches!(
                index.node_projection(
                    parser
                        .root_node()
                        .children()
                        .last()
                        .unwrap()
                        .as_tree_sitter()
                ),
                Err(OwnerError::UnsupportedOwner)
            ));
            drop(index);
            drop(parser);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "cpp")]
#[test]
fn exact_signatures_preserve_tokens_scope_and_complex_competitors() {
    for (source, reason, count) in [
        (
            "struct intx {};\nvoid f(intx);\n/**@fn void f(int x)*/",
            Some(UnresolvedReason::MissingName),
            0,
        ),
        (
            "typedef int intx;\nvoid f(int x);\n/**@fn void f(intx)*/",
            Some(UnresolvedReason::MissingName),
            0,
        ),
        (
            "struct intx {};\nvoid f(intx); void f(int x);\n/**@fn void f(int x)*/",
            None,
            1,
        ),
        (
            "void f ( int /*type*/ x );\n/**@fn void f(int x)*/",
            None,
            1,
        ),
        (
            "namespace A { void f(int x); }\n/**@fn void f(int x)*/",
            Some(UnresolvedReason::MissingName),
            0,
        ),
        (
            "namespace A { void f(int x); /**@fn void f(int x)*/ }\nnamespace B { void f(int x); }",
            None,
            1,
        ),
        (
            "void f(int x); void f(int x = [] { return 1; }());\n/**@fn void f(int x)*/",
            Some(UnresolvedReason::Ambiguous),
            2,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
        assert!(!parser.root_node().has_error(), "{source}");
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().resolution,
            reason.map_or(Resolution::Resolved, Resolution::Unresolved),
            "{source}"
        );
        assert_eq!(association.get().owner_count(), count, "{source}");
    }
    for source in [
        "/**own*/\nvoid f(int x = [] { return 1; }());\n/**@fn void f(int x)*/",
        "void f();\n/**@fn void f(int x = 1)*/",
        "namespace A { void f(); }\n/**@fn void A::f()*/",
    ] {
        let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let reference = index.documents().last().unwrap();
        assert!(
            index
                .assessments()
                .any(|a| index.assessed_document(a).unwrap() == Some(reference)),
            "{source}"
        );
        assert!(
            !index
                .associations()
                .any(
                    |a| index.facts().associated_document(a).unwrap() == reference
                        && a.get().resolution == Resolution::Resolved
                )
        );
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn qualified_names(language: LANG) {
    for declaration in ["void f(void);", "void f(void) {}"] {
        let source = format!("/**@fn void f(void)*/\n#if FLAG\n{declaration}\n#endif");
        let parser = AnyParser::parse(language, source.into_bytes(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().resolution,
            Resolution::Unresolved(UnresolvedReason::Conditional)
        );
        assert_eq!(association.get().owner_count(), 1);
    }
}
#[cfg(feature = "c")]
#[test]
fn c_named_candidates_retain_conditional_provenance() {
    qualified_names(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_named_candidates_retain_conditional_provenance() {
    qualified_names(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_named_candidates_retain_conditional_provenance() {
    qualified_names(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_named_candidates_retain_conditional_provenance() {
    qualified_names(LANG::Objc);
}

#[cfg(feature = "cpp")]
#[test]
fn nested_default_lambda_signature_work_is_linear_in_primary_facts() {
    for depth in [8, 16, 32, 64, 128] {
        let source = format!(
            "{}void leaf();{}",
            "void f(int x = [] { ".repeat(depth),
            " return 1; }());".repeat(depth)
        );
        let parser = AnyParser::parse(LANG::Cpp, source.into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        assert_eq!(index.documents().len(), 0);
        assert_eq!(
            index
                .assessments()
                .filter(|a| a.get().reason == "Complex signature awaits exact named matching")
                .count(),
            depth
        );
        // Each complex declaration stores one name; leaf stores its name and token key.
        assert_eq!(after.key_bytes_stored - before.key_bytes_stored, depth + 17);
        assert_eq!(after.name_bytes - before.name_bytes, 3 * depth + 60);
    }
}

#[cfg(any(feature = "go", feature = "ruby", feature = "java"))]
fn boundary_members(language: LANG, source: &str, expected: &str) {
    let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    let text = index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(text.text(), expected);
    let mut end = 0;
    for portion in text.origins(0..expected.len()).unwrap() {
        assert_eq!(portion.output.start, end);
        end = portion.output.end;
    }
    assert_eq!(end, expected.len());
}
#[cfg(feature = "go")]
#[test]
fn go_trims_logical_boundary_members_only() {
    boundary_members(
        LANG::Go,
        "package p\n//\n// audit\n//\nfunc F() {}",
        "audit",
    );
    boundary_members(LANG::Go, "package p\n// a\n//\n// b\nfunc F() {}", "a\n\nb");
    boundary_members(LANG::Go, "package p\n//\n//\nfunc F() {}", "");
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_trims_logical_boundary_members_only() {
    boundary_members(LANG::Ruby, "#\n# audit\n#\ndef f; end", "audit");
}
#[cfg(feature = "java")]
#[test]
fn java_trims_logical_boundary_members_only() {
    boundary_members(LANG::Java, "///\n///audit\n///\nclass C {}", "audit");
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn inline_cost(language: LANG) {
    let mut counts = Vec::new();
    for size in [8, 16, 32, 64, 128] {
        let source = format!("/**{}*/\nvoid f() {{}}", "`x` ".repeat(size));
        let parser = AnyParser::parse(language, source.into_bytes(), None, None).unwrap();
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let after = observations();
        counts.push((
            after.classifier_bytes - before.classifier_bytes,
            after.structural_bytes - before.structural_bytes,
        ));
    }
    for pair in counts.windows(3) {
        assert_eq!(pair[2].0 - pair[1].0, 2 * (pair[1].0 - pair[0].0));
        assert_eq!(pair[2].1 - pair[1].1, 2 * (pair[1].1 - pair[0].1));
    }
}
#[cfg(feature = "c")]
#[test]
fn c_inline_code_scan_is_linear() {
    inline_cost(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_inline_code_scan_is_linear() {
    inline_cost(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_inline_code_scan_is_linear() {
    inline_cost(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_inline_code_scan_is_linear() {
    inline_cost(LANG::Objc);
}

#[cfg(feature = "cpp")]
#[test]
fn gathered_disposition_does_not_rescan_structural_member_per_fragment() {
    let mut counts = Vec::new();
    for size in [8, 16, 32, 64, 128] {
        let source = format!("{}/**{}@file*/", "/**a*/\n".repeat(size), "x ".repeat(size));
        let parser = AnyParser::parse(LANG::Cpp, source.into_bytes(), None, None).unwrap();
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(
            index.documents().next().unwrap().get().fragment_count(),
            size + 1
        );
        counts.push(observations().structural_bytes - before.structural_bytes);
    }
    for triple in counts.windows(3) {
        assert_eq!(triple[2] - triple[1], 2 * (triple[1] - triple[0]));
    }
}

#[cfg(feature = "java")]
#[test]
fn multiline_text_reuses_retained_fragment_intersections() {
    for size in [32, 64, 128, 256] {
        let source = format!("/**\n{}*/\nclass C {{}}", " * é\n".repeat(size));
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANG::Java.tree_sitter_language().unwrap())
            .unwrap();
        let mut point = tree_sitter::Point::new(0, 0);
        let ranges: Vec<_> = source
            .bytes()
            .enumerate()
            .map(|(at, byte)| {
                let start_point = point;
                if byte == b'\n' {
                    point.row += 1;
                    point.column = 0;
                } else {
                    point.column += 1;
                }
                tree_sitter::Range {
                    start_byte: at,
                    end_byte: at + 1,
                    start_point,
                    end_point: point,
                }
            })
            .collect();
        parser.set_included_ranges(&ranges).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let before = observations();
        let index = collect(
            LANG::Java,
            &tree,
            source.as_bytes(),
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
        let after = observations();
        let text = index
            .complete_text(index.documents().next().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            text.text(),
            std::iter::repeat_n("é", size)
                .collect::<Vec<_>>()
                .join("\n")
        );
        // One fragment intersection binary-searches for byte zero; text emission performs none.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            (usize::BITS - source.len().leading_zeros()) as usize
        );
        assert_eq!(text.origins(0..2).unwrap().count(), 2);
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn export_and_binding(language: LANG) {
    let source = "/**audit*/\nexport function f() {}";
    let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
    for index in [
        parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap(),
        collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap(),
    ] {
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().binding.unwrap().node().kind(),
            "export_statement"
        );
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("function")
        };
        assert_eq!(node.node().kind(), "function_declaration");
        assert_eq!(node.node().byte_range(), 18..33);
        assert_eq!(
            index
                .complete_text(index.documents().next().unwrap())
                .unwrap()
                .unwrap()
                .text(),
            "audit"
        );
    }
    let parser = AnyParser::parse(
        language,
        b"/** @file audit */\nfunction f() {}".to_vec(),
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
    assert!(index.associations().next().unwrap().get().binding.is_none());
}
#[cfg(feature = "javascript")]
#[test]
fn javascript_export_projection_and_file_binding_are_exact() {
    export_and_binding(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_export_projection_and_file_binding_are_exact() {
    export_and_binding(LANG::Mozjs);
}

#[cfg(feature = "rust")]
#[test]
fn rust_raw_two_fragment_smoke_and_file_binding() {
    let source = b"///a\n///b\nfn f() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let index = collect(
        LANG::Rust,
        parser.ts_tree(),
        source,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    let doc = index.documents().next().unwrap();
    assert_eq!(doc.get().fragment_count(), 2);
    let text = index.complete_text(doc).unwrap().unwrap();
    assert_eq!(text.text(), "a\nb");
    let portions: Vec<_> = text.origins(0..3).unwrap().collect();
    assert_eq!(portions.len(), 3);
    let OriginView::Copied(a) = &portions[0].origin else {
        panic!("copy")
    };
    let OriginView::Copied(b) = &portions[2].origin else {
        panic!("copy")
    };
    assert_eq!(a.range(), 3..4);
    assert_eq!(b.range(), 8..9);
    assert_eq!(portions[1].origin, OriginView::Synthetic);
    let parser = AnyParser::parse(
        LANG::Rust,
        b"//! crate\n/// item\nfn f() {}".to_vec(),
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
    let associations: Vec<_> = index.associations().collect();
    assert_eq!(associations.len(), 2);
    assert!(associations[0].get().binding.is_none());
    assert_eq!(
        associations[1].get().binding.unwrap().node().byte_range(),
        19..28
    );
}

#[cfg(feature = "c")]
#[test]
fn trailing_binding_is_the_preceding_declaration() {
    let parser = AnyParser::parse(
        LANG::C,
        b"struct S { int x; ///<audit\nint y; };".to_vec(),
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
    assert_eq!(
        index
            .associations()
            .next()
            .unwrap()
            .get()
            .binding
            .unwrap()
            .node()
            .byte_range(),
        11..17
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_file_opener_has_no_following_binding() {
    let parser = AnyParser::parse(
        LANG::Lua,
        b"---audit\nfunction f() end".to_vec(),
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
    assert!(index.associations().next().unwrap().get().binding.is_none());
}

#[cfg(feature = "rust")]
#[test]
fn rust_macro_documents_do_not_coalesce_with_concrete_targets() {
    let source = "///outer\nfn f() { m! { ///macro\nfn f() {} } }";
    let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 2);
    let associations: Vec<_> = index.associations().collect();
    assert_eq!(associations[0].get().resolution, Resolution::Resolved);
    assert_eq!(
        associations[1].get().resolution,
        Resolution::Unresolved(UnresolvedReason::Expansion)
    );
    assert_eq!(associations[1].get().owner_count(), 0);
    assert!(associations[1].get().binding.is_none());
}

#[cfg(feature = "cpp")]
#[test]
fn recovered_named_candidate_keeps_grammar_qualification() {
    let source = "/**@fn f*/\nvoid f(int x = );";
    let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().resolution,
        Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
    );
    assert_eq!(association.get().owner_count(), 1);
}

#[cfg(all(feature = "go", feature = "ruby"))]
#[test]
fn source_scope_folds_compute_and_search_positive_states() {
    for size in [8, 16, 32] {
        for (language, source) in [
            (
                LANG::Go,
                format!(
                    "package p\ntype S struct {{\n{}X int\n{} }}\n",
                    "X struct {\n".repeat(size),
                    "}\n".repeat(size)
                ),
            ),
            (
                LANG::Ruby,
                format!(
                    "{}# audit\ndef f; end\n{}",
                    "class C\n".repeat(size),
                    "end\n".repeat(size)
                ),
            ),
        ] {
            let parser = AnyParser::parse(language, source.into_bytes(), None, None).unwrap();
            assert!(!parser.root_node().has_error());
            let before = observations();
            let steps = crate::node::fold_steps::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let after = observations();
            let (expected_steps, expected_probes) = if language == LANG::Go {
                (3 * size + 4, 4 * size + 5)
            } else {
                (2 * size, 4 * size - 1)
            };
            // Each nested Go field adds struct/list/field levels; Ruby adds class/body.
            assert_eq!(crate::node::fold_steps::observed() - steps, expected_steps);
            assert_eq!(
                after.fold_slot_probes - before.fold_slot_probes,
                expected_probes
            );
            assert_eq!(index.documents().len(), usize::from(language == LANG::Ruby));
        }
    }
}
