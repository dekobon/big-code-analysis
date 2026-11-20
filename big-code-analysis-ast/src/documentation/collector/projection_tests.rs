use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn splices(language: LANG) {
    for (source, expected, copied) in [
        ("/**a\\\nb*/\nvoid f() {}", "ab", vec![3..4, 6..7]),
        ("/**a\\\r\nb*/\nvoid f() {}", "ab", vec![3..4, 7..8]),
        ("///a\\\nb\nvoid f() {}", "ab", vec![3..4, 6..7]),
        ("///a\\\\\nb\nvoid f() {}", "a\\b", vec![3..5, 7..8]),
        ("///a\\\n///b\nvoid f() {}", "a///b", vec![3..4, 6..10]),
    ] {
        let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1, "{language:?}: {source}");
        let text = index
            .complete_text(index.documents().next().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(text.text(), expected, "{language:?}: {source}");
        let origins: Vec<_> = text
            .origins(0..text.text().len())
            .unwrap()
            .map(|portion| {
                let OriginView::Copied(piece) = portion.origin else {
                    panic!("copied splice piece")
                };
                piece.range()
            })
            .collect();
        assert_eq!(origins, copied);
    }
}

#[cfg(feature = "c")]
#[test]
fn c_phase_two_line_and_block_maps() {
    splices(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_phase_two_line_and_block_maps() {
    splices(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_phase_two_line_and_block_maps() {
    splices(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_phase_two_line_and_block_maps() {
    splices(LANG::Objc);
}

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn structural_file(language: LANG, source: &str, expected: &str) {
    let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    let document = index.documents().next().unwrap();
    assert_eq!(document.get().role(), DocumentRole::FilePackage);
    assert_eq!(
        index.complete_text(document).unwrap().unwrap().text(),
        expected
    );
    let association = index.associations().next().unwrap();
    assert!(matches!(
        index
            .facts()
            .associated_owner(association, 0)
            .unwrap()
            .get(),
        Owner::File(_)
    ));
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_file_tags_override_following_declaration() {
    structural_file(
        LANG::Javascript,
        "/**@file audit*/\nfunction f() {}",
        "@file audit",
    );
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_file_tags_override_following_declaration() {
    structural_file(
        LANG::Mozjs,
        "/**@file audit*/\nfunction f() {}",
        "@file audit",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_file_tags_override_following_declaration() {
    structural_file(
        LANG::Typescript,
        "/** @packageDocumentation */\nexport const x = 1;",
        "@packageDocumentation",
    );
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_file_tags_override_following_declaration() {
    structural_file(
        LANG::Tsx,
        "/** @packageDocumentation */\nexport const x = 1;",
        "@packageDocumentation",
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_block_wrapper_projects_following_function() {
    let parser = AnyParser::parse(
        LANG::Lua,
        b"module(\"m\")\ndo\n---audit\nfunction f() end\nend".to_vec(),
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
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("function owner")
    };
    assert_eq!(node.node().kind(), "function_declaration");
}

#[cfg(feature = "cpp")]
#[test]
fn doxygen_file_gathering_is_one_document_and_page_remains_pending() {
    for (source, file) in [
        ("/**license*/\n/**@file*/", true),
        ("/**@page p Audit*/\nvoid f();", false),
    ] {
        let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), usize::from(file));
        if file {
            let doc = index.documents().next().unwrap();
            assert_eq!(doc.get().fragment_count(), 2);
            assert_eq!(
                index.complete_text(doc).unwrap().unwrap().text(),
                "license\n@file"
            );
        } else {
            assert!(
                index
                    .assessments()
                    .any(|assessment| assessment.get().axis == AssessmentAxis::Owner)
            );
        }
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn incomplete_declarators(language: LANG) {
    for source in [
        "/**audit*/\nint a, (*callback)(void);",
        "/**audit*/\nint (*only)(void);",
        "/**audit*/\nint a, f(void);",
    ] {
        let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let declaration = parser.root_node().children().last().unwrap();
        assert!(
            index
                .node_associations(declaration.as_tree_sitter())
                .unwrap()
                .projection_pending(),
            "{source}"
        );
        assert_eq!(index.associations().len(), 0, "{source}");
        assert_eq!(
            index.facts().owners().len(),
            1,
            "only the file owner is established"
        );
        let document = index.documents().next().unwrap();
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "audit"
        );
        assert!(
            index
                .assessments()
                .any(|assessment| index.assessed_document(assessment).unwrap() == Some(document))
        );
    }
    for source in [
        "void f(void);\n/**lead*/\n/**@fn void f(void)*/",
        "void f(void) {}\n/**lead*/\n/**@fn void f(void)*/",
    ] {
        let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(document.get().fragment_count(), 2);
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "lead\n@fn void f(void)"
        );
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().owner_count(), 1);
    }
}

#[cfg(feature = "c")]
#[test]
fn c_incomplete_declarators_and_gathered_names() {
    incomplete_declarators(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_incomplete_declarators_and_gathered_names() {
    incomplete_declarators(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_incomplete_declarators_and_gathered_names() {
    incomplete_declarators(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_incomplete_declarators_and_gathered_names() {
    incomplete_declarators(LANG::Objc);
}
