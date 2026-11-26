//! Collects every enabled LANG variant's primary documentation through the root Ast
//! and checks independent inventory rows and discriminating in-memory controls.

#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
use big_code_analysis::{Ast, LANG, Source};
#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
use big_code_analysis_ast::documentation::{
    Capability, CollectionError, DocumentationContext, DocumentationIndex, DocumentationOptions,
    Owner, Profile, Resolution,
};
#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
use big_code_analysis_ast::test_support::documentation::{
    assert_primary_extraction, primary, source,
};

#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
fn projection(index: &DocumentationIndex<'_>) -> serde_json::Value {
    let facts = index.facts();
    serde_json::Value::Array(
        index
            .documents()
            .map(|document| {
                let fragments: Vec<_> = (0..document.get().fragment_count())
                    .map(|at| {
                        let fragment = facts.document_fragment(document, at).unwrap();
                        let [piece] = fragment.get().source().pieces() else {
                            panic!("one source piece per fragment")
                        };
                        let range = piece.range();
                        serde_json::json!([range.start, range.end])
                    })
                    .collect();
                let text = index
                    .complete_text(document)
                    .unwrap()
                    .map(|complete| complete.text().to_owned());
                let associations: Vec<_> = index
                    .associations()
                    .filter(|association| {
                        facts.associated_document(*association).unwrap() == document
                    })
                    .collect();
                let [association] = associations.as_slice() else {
                    panic!("one association per document")
                };
                let node = if association.get().resolution == Resolution::Resolved
                    && association.get().owner_count() == 1
                {
                    match facts.associated_owner(*association, 0).unwrap().get() {
                        Owner::Node(declared) => Some(declared.node()),
                        _ => None,
                    }
                } else {
                    None
                };
                let owner = node.map(|node| {
                    let range = node.byte_range();
                    serde_json::json!([range.start, range.end])
                });
                let owner_kind = node.map(|node| node.kind());
                serde_json::json!({
                    "fragments": fragments,
                    "text": text,
                    "owner": owner,
                    "owner_kind": owner_kind,
                })
            })
            .collect(),
    )
}

#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
#[derive(Clone, Copy)]
enum Expectation {
    Row(Profile),
    Rejected(Profile),
    Helper,
}

#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
fn check_policy(ast: &Ast, lang: LANG, expected: Expectation, filename: Option<&str>) {
    let result = ast.documentation(
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    );
    match expected {
        Expectation::Row(profile) => {
            let index = result.unwrap();
            assert_eq!(index.profile(), profile, "{lang}");
            assert_eq!(index.capability(), Capability::Partial, "{lang}");
            assert_eq!(index.assessments().len(), 0, "{lang}");
            if let Some(filename) = filename {
                assert_eq!(
                    index.context().logical_filename.as_deref(),
                    Some(filename),
                    "{lang}"
                );
            }
            assert_primary_extraction(lang, Ok(projection(&index)));
        }
        Expectation::Rejected(profile) => {
            assert_eq!(
                result.err(),
                Some(CollectionError::AdapterUnavailable {
                    language: lang,
                    profile
                }),
                "{lang}"
            );
        }
        Expectation::Helper => {
            let index = result.unwrap();
            assert_eq!(index.profile(), Profile::Helper, "{lang}");
            assert_eq!(index.capability(), Capability::NoDeclarationModel, "{lang}");
            assert_eq!(index.documents().len(), 0, "{lang}");
            assert_eq!(index.associations().len(), 0, "{lang}");
            assert_eq!(index.assessments().len(), 0, "{lang}");
            assert_primary_extraction(lang, Err("NoDeclarationModel"));
        }
    }
}

#[cfg(any(
    feature = "bash",
    feature = "c",
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
    feature = "typescript",
    feature = "c-family-helpers"
))]
#[test]
fn every_language_variant_meets_its_documentation_policy_through_the_root_ast() {
    use Expectation::{Helper, Rejected, Row};
    let directory = tempfile::tempdir().unwrap();
    let mut counts = (0, 0, 0);
    for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
        let expected = match lang {
            LANG::Rust => Row(Profile::Rust),
            LANG::Python => Rejected(Profile::Python),
            LANG::Java => Row(Profile::Javadoc),
            LANG::Kotlin => Row(Profile::Kdoc),
            LANG::Groovy => Row(Profile::Groovydoc),
            LANG::Csharp => Row(Profile::CsharpXml),
            LANG::Php => Row(Profile::BcaPhpSource),
            LANG::Javascript | LANG::Mozjs => Row(Profile::Jsdoc),
            LANG::Typescript | LANG::Tsx => Row(Profile::BcaTypescriptSource),
            LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => Row(Profile::Doxygen),
            LANG::Go => Row(Profile::GoDoc),
            LANG::Ruby => Row(Profile::Yard),
            LANG::Lua => Row(Profile::Ldoc),
            LANG::Perl => Rejected(Profile::Pod),
            LANG::Elixir => Rejected(Profile::Elixir),
            LANG::Bash => Rejected(Profile::Shdoc),
            LANG::Tcl => Rejected(Profile::Ruff),
            LANG::Irules => Rejected(Profile::Irules),
            LANG::Ccomment | LANG::Preproc => Helper,
        };
        match expected {
            Row(_) => counts.0 += 1,
            Rejected(_) => counts.1 += 1,
            Helper => counts.2 += 1,
        }
        let filename = primary(lang);
        let bytes = source(filename);
        let ast = Ast::parse(Source::new(lang, &bytes)).unwrap();
        assert_eq!(ast.language(), lang);
        check_policy(&ast, lang, expected, None);
        // Mozjs, Mozcpp, Ccomment and Preproc have no filename that selects their grammar.
        if !matches!(
            lang,
            LANG::Mozjs | LANG::Mozcpp | LANG::Ccomment | LANG::Preproc
        ) {
            let path = directory.path().join(filename);
            std::fs::write(&path, &bytes).unwrap();
            let from_path = Ast::from_path(&path).unwrap();
            assert_eq!(from_path.language(), lang);
            check_policy(&from_path, lang, expected, Some(filename));
        }
    }
    let ran = counts.0 + counts.1 + counts.2;
    assert_eq!(ran, LANG::into_enum_iter().filter(LANG::is_enabled).count());
    assert!(ran > 0);
    #[cfg(feature = "all-languages")]
    assert_eq!(counts, (17, 6, 2));
}

#[cfg(feature = "rust")]
#[test]
fn the_projection_tells_rust_results_apart() {
    let controls = [
        // expected: the primary comment is owned by the following function.
        (
            "///audit\nfn f() {}",
            serde_json::json!([{"fragments":[[0,8]],"owner":[9,18],"owner_kind":"function_item","text":"audit"}]),
        ),
        // expected: the same ranges but other comment text.
        (
            "///other\nfn f() {}",
            serde_json::json!([{"fragments":[[0,8]],"owner":[9,18],"owner_kind":"function_item","text":"other"}]),
        ),
        // expected: a longer declaration moves the owner's end to byte 23.
        (
            "///audit\nfn g(a: u8) {}",
            serde_json::json!([{"fragments":[[0,8]],"owner":[9,23],"owner_kind":"function_item","text":"audit"}]),
        ),
        // expected: two distinct documents are returned in source order.
        (
            "///audit\nfn f() {}\n///b\nfn g() {}",
            serde_json::json!([
                {"fragments":[[0,8]],"owner":[9,18],"owner_kind":"function_item","text":"audit"},
                {"fragments":[[19,23]],"owner":[24,33],"owner_kind":"function_item","text":"b"}
            ]),
        ),
        // expected: no comment means no document.
        ("fn f() {}", serde_json::json!([])),
        // expected: the computed #[doc = concat!("a")] value has no complete text.
        (
            "#[doc = concat!(\"a\")]\nfn f() {}",
            serde_json::json!([{"fragments":[[0,21]],"owner":[22,31],"owner_kind":"function_item","text":null}]),
        ),
        // expected: the inner comment belongs to the file, not a node owner.
        (
            "//!audit\nfn f() {}",
            serde_json::json!([{"fragments":[[0,8]],"owner":null,"owner_kind":null,"text":"audit"}]),
        ),
        // expected: two comment rows make two fragments but one complete text.
        (
            "///audit\n///b\nfn f() {}",
            serde_json::json!([{"fragments":[[0,8],[9,13]],"owner":[14,23],"owner_kind":"function_item","text":"audit\nb"}]),
        ),
    ];
    for (bytes, expected) in controls {
        let ast = Ast::parse(Source::new(LANG::Rust, bytes.as_bytes())).unwrap();
        let index = ast
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(projection(&index), expected, "{bytes}");
    }
}

#[cfg(feature = "cpp")]
#[test]
fn the_projection_names_no_owner_for_an_unresolved_or_slot_association() {
    use big_code_analysis_ast::documentation::UnresolvedReason;
    let controls = [
        (
            "/**@fn void g()\n * audit*/\nvoid f();",
            serde_json::json!([{"fragments":[[0,26]],"owner":null,"owner_kind":null,"text":"@fn void g()\naudit"}]),
        ),
        (
            "/**a*/\nint x, y;",
            serde_json::json!([{"fragments":[[0,6]],"owner":null,"owner_kind":null,"text":"a"}]),
        ),
    ];
    for (at, (bytes, expected)) in controls.into_iter().enumerate() {
        let ast = Ast::parse(Source::new(LANG::Cpp, bytes.as_bytes())).unwrap();
        let index = ast
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(projection(&index), expected, "{bytes}");
        let associations: Vec<_> = index.associations().collect();
        let [association] = associations.as_slice() else {
            panic!("one association per C++ control")
        };
        if at == 0 {
            assert_eq!(
                association.get().resolution,
                Resolution::Unresolved(UnresolvedReason::MissingName)
            );
        } else {
            assert_eq!(association.get().resolution, Resolution::Resolved);
            assert_eq!(association.get().owner_count(), 2);
            for ordinal in 0..2 {
                let owner = index
                    .facts()
                    .associated_owner(*association, ordinal)
                    .unwrap();
                assert!(matches!(owner.get(), Owner::Slot(_)));
            }
        }
    }
}
