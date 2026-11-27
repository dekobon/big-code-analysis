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
    assert_primary_extraction, inventory, primary, source,
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
                        Owner::Group(group)
                            if index.profile() == Profile::Elixir && group.pieces.len() == 1 =>
                        {
                            Some(group.pieces[0].node.unwrap().node())
                        }
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
    // The inventory row names the owner of a profile that is not the default one.
    FileDocument(Profile),
    Rejected(Profile),
    Helper,
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
    feature = "typescript",
))]
fn check_file_document(index: &DocumentationIndex<'_>, lang: LANG) {
    let row = &inventory()[lang.to_string()];
    assert_eq!(
        projection(index),
        serde_json::json!([{
            "fragments": row["fragments"], "text": row["text"],
            "owner": null, "owner_kind": null
        }]),
        "{lang}"
    );
    let associations: Vec<_> = index.associations().collect();
    let [association] = associations.as_slice() else {
        panic!("{lang}: one association")
    };
    assert_eq!(association.get().resolution, Resolution::Resolved);
    assert_eq!(association.get().owner_count(), 1);
    let owner = index.facts().associated_owner(*association, 0).unwrap();
    assert!(matches!(owner.get(), Owner::File(_)), "{lang}");
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
        Expectation::Row(profile) | Expectation::FileDocument(profile) => {
            let index = result.unwrap();
            assert_eq!(index.profile(), profile, "{lang}");
            assert_eq!(index.capability(), Capability::Partial, "{lang}");
            assert_eq!(index.assessments().len(), 0, "{lang}");
            assert_eq!(
                index.context().logical_filename.as_deref(),
                filename,
                "{lang}"
            );
            if matches!(expected, Expectation::Row(_)) {
                assert_primary_extraction(lang, Ok(projection(&index)));
            } else {
                check_file_document(&index, lang);
            }
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
    use Expectation::{FileDocument, Helper, Rejected, Row};
    let directory = tempfile::tempdir().unwrap();
    let mut counts = (0, 0, 0, 0);
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
            LANG::Perl => FileDocument(Profile::Pod),
            LANG::Elixir => Row(Profile::Elixir),
            LANG::Bash => Rejected(Profile::Shdoc),
            LANG::Tcl => Row(Profile::Ruff),
            LANG::Irules => Rejected(Profile::Irules),
            LANG::Ccomment | LANG::Preproc => Helper,
        };
        match expected {
            Row(_) => counts.0 += 1,
            Rejected(_) => counts.1 += 1,
            Helper => counts.2 += 1,
            FileDocument(_) => counts.3 += 1,
        }
        let filename = primary(lang);
        let bytes = source(filename);
        let ast = Ast::parse(Source::new(lang, &bytes)).unwrap();
        assert_eq!(ast.language(), lang);
        check_policy(&ast, lang, expected, None);
        // Mozcpp, Ccomment and Preproc have no filename that selects their grammar.
        if !matches!(lang, LANG::Mozcpp | LANG::Ccomment | LANG::Preproc) {
            // The fixture is shared with Javascript; `.jsm` is the extension Mozjs owns.
            let filename = if lang == LANG::Mozjs {
                "primary.jsm"
            } else {
                filename
            };
            let path = directory.path().join(filename);
            std::fs::write(&path, &bytes).unwrap();
            let from_path = Ast::from_path(&path).unwrap();
            assert_eq!(from_path.language(), lang);
            check_policy(&from_path, lang, expected, Some(filename));
        }
    }
    let ran = counts.0 + counts.1 + counts.2 + counts.3;
    assert_eq!(ran, LANG::into_enum_iter().filter(LANG::is_enabled).count());
    assert!(ran > 0);
    #[cfg(feature = "all-languages")]
    assert_eq!(counts, (19, 3, 2, 1));
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
        if bytes.starts_with("//!") {
            let association = index.associations().next().unwrap();
            assert_eq!(association.get().resolution, Resolution::Resolved);
            let owner = index.facts().associated_owner(association, 0).unwrap();
            assert!(matches!(owner.get(), Owner::File(_)), "{bytes}");
        }
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
#[test]
fn every_perturbation_manifest_anchor_matches_uniquely() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut manifests = 0;
    let mut cases = 0;
    for directory in ["utils", "fuzz"] {
        for entry in std::fs::read_dir(root.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if !name.contains("perturb") || path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let Some(rows) = manifest.get("cases").and_then(serde_json::Value::as_array) else {
                panic!("{} has no cases array", path.display());
            };
            let manifest_root = root.join(
                manifest
                    .get("cwd")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("."),
            );
            assert!(!rows.is_empty(), "{}", path.display());
            manifests += 1;
            for row in rows {
                let target = manifest_root.join(row["file"].as_str().unwrap());
                let old = row["old"].as_str().unwrap();
                let new = row["new"].as_str().unwrap();
                let content = std::fs::read_to_string(&target).unwrap();
                assert!(!old.is_empty(), "{}: {}", path.display(), target.display());
                assert_ne!(old, new, "{}: {}", path.display(), target.display());
                assert_eq!(
                    content.matches(old).count(),
                    1,
                    "{}: {}: {}",
                    path.display(),
                    row["name"],
                    target.display()
                );
                cases += 1;
            }
        }
    }
    assert!(manifests > 0 && cases > 0);
}

// test-lang-gates: hand-written(c,cpp,mozcpp,objc,rust,java,lua,javascript,mozjs,groovy,csharp,php) — fixture filename selects enabled parser.
#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc",
    feature = "rust",
    feature = "java",
    feature = "lua",
    feature = "javascript",
    feature = "mozjs",
    feature = "groovy",
    feature = "csharp",
    feature = "php",
    feature = "python",
    feature = "perl",
    feature = "tcl",
    feature = "irules"
))]
fn form_languages(row: &serde_json::Value) -> Vec<LANG> {
    let file = row.get("path").and_then(serde_json::Value::as_str).unwrap();
    if let Some(names) = row.get("languages").and_then(serde_json::Value::as_array) {
        return names
            .iter()
            .map(|name| match name.as_str().unwrap() {
                "javascript" => LANG::Javascript,
                "mozjs" => LANG::Mozjs,
                "c" => LANG::C,
                "cpp" => LANG::Cpp,
                "mozcpp" => LANG::Mozcpp,
                "objc" => LANG::Objc,
                other => panic!("unknown form language {other}"),
            })
            .collect();
    }
    vec![match file.rsplit('.').next().unwrap() {
        "rs" => LANG::Rust,
        "py" => LANG::Python,
        "java" => LANG::Java,
        "lua" => LANG::Lua,
        "php" => LANG::Php,
        "pl" => LANG::Perl,
        "tcl" => LANG::Tcl,
        "groovy" => LANG::Groovy,
        "cs" => LANG::Csharp,
        other => panic!("unknown form extension {other}"),
    }]
}

// test-lang-gates: hand-written(c,cpp,mozcpp,objc,rust,java,lua,javascript,mozjs,groovy,csharp,php) — only enabled fixture rows call this helper.
#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc",
    feature = "rust",
    feature = "java",
    feature = "lua",
    feature = "javascript",
    feature = "mozjs",
    feature = "groovy",
    feature = "csharp",
    feature = "php",
    feature = "python",
    feature = "perl",
    feature = "tcl",
    feature = "irules"
))]
#[allow(clippy::too_many_lines)]
fn check_form_documents(
    index: &DocumentationIndex<'_>,
    source: &[u8],
    promised: &[serde_json::Value],
    path: &str,
) {
    use big_code_analysis_ast::documentation::UnresolvedReason;
    let docs: Vec<_> = index.documents().collect();
    assert_eq!(docs.len(), promised.len(), "{path}");
    for (doc, expected) in docs.into_iter().zip(promised) {
        let fragments: Vec<_> = (0..doc.get().fragment_count())
            .map(|at| {
                let fragment = index.facts().document_fragment(doc, at).unwrap();
                let pieces = fragment.get().source().pieces();
                assert_eq!(pieces.len(), 1, "{path}");
                let span = pieces[0].range();
                assert!(span.start <= span.end && span.end <= source.len(), "{path}");
                [span.start, span.end]
            })
            .collect();
        if let Some(ranges) = expected.get("fragments") {
            assert_eq!(serde_json::json!(fragments), *ranges, "{path}");
        } else {
            let spellings: Vec<&str> = expected
                .get("fragment_spellings")
                .and_then(serde_json::Value::as_array)
                .map_or_else(
                    || vec![expected["fragment_spelling"].as_str().unwrap()],
                    |array| array.iter().map(|v| v.as_str().unwrap()).collect(),
                );
            assert_eq!(fragments.len(), spellings.len(), "{path}");
            for (span, spelling) in fragments.iter().zip(spellings) {
                assert_eq!(&source[span[0]..span[1]], spelling.as_bytes(), "{path}");
            }
        }
        let text = index.complete_text(doc).unwrap();
        if let Some(value) = expected.get("text") {
            assert_eq!(text.unwrap().text(), value.as_str().unwrap(), "{path}");
        } else if expected
            .get("availability")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|v| v.starts_with("unavailable"))
        {
            assert!(text.is_none(), "{path}");
        }
        let associations: Vec<_> = index
            .associations()
            .filter(|association| index.facts().associated_document(*association).unwrap() == doc)
            .collect();
        assert_eq!(associations.len(), 1, "{path}");
        let association = associations[0];
        let resolution = match expected
            .get("association")
            .and_then(serde_json::Value::as_str)
        {
            Some("unresolved macro expansion") => {
                Resolution::Unresolved(UnresolvedReason::Expansion)
            }
            Some("resolved") | None if expected.get("owner").is_some_and(|v| !v.is_null()) => {
                Resolution::Resolved
            }
            Some(other) => panic!("{path}: unknown promised association {other}"),
            None => Resolution::Unattached,
        };
        assert_eq!(association.get().resolution, resolution, "{path}");
        if let Some(owner) = expected.get("owner") {
            if owner.is_null() {
                assert_eq!(association.get().owner_count(), 0, "{path}");
            } else if owner == "file" {
                assert_eq!(association.get().owner_count(), 1, "{path}");
                let binding = index.facts().associated_owner(association, 0).unwrap();
                assert!(matches!(binding.get(), Owner::File(_)), "{path}");
            } else if let Some(range) = owner.as_array() {
                assert_eq!(association.get().owner_count(), 1, "{path}");
                let node = index.facts().associated_owner(association, 0).unwrap();
                let Owner::Node(node) = node.get() else {
                    panic!("{path}: expected node owner")
                };
                assert_eq!(
                    serde_json::json!([node.node().start_byte(), node.node().end_byte()]),
                    serde_json::Value::Array(range.clone()),
                    "{path}"
                );
            } else {
                assert_eq!(association.get().owner_count(), 1, "{path}");
                let name = owner.as_str().unwrap();
                let binding = index.facts().associated_owner(association, 0).unwrap();
                if path == "groovy-spaces.groovy" && name == "field C.x" {
                    let Owner::Slot(slot) = binding.get() else {
                        panic!("{path}: expected field slot")
                    };
                    assert_eq!(
                        slot.role,
                        big_code_analysis_ast::documentation::SlotRole::Field
                    );
                    assert_eq!(slot.ordinal, 0);
                    assert_eq!(slot.source.pieces()[0].range(), 107..108);
                    let Owner::Node(declaration) =
                        index.facts().slot_enclosing(binding).unwrap().get()
                    else {
                        panic!("{path}: expected enclosing field declaration")
                    };
                    assert_eq!(declaration.node().byte_range(), 103..108);
                } else {
                    let Owner::Node(node) = binding.get() else {
                        panic!("{path}: expected node owner")
                    };
                    let span = node.node().byte_range();
                    assert!(span.end <= source.len(), "{path}");
                    let bytes = &source[span];
                    let spelling = match (path, name) {
                        ("groovy-spaces.groovy", "constructor C()") => "C()",
                        ("groovy-spaces.groovy", "method C.f") => "def f()",
                        _ => name,
                    };
                    assert!(
                        bytes.starts_with(spelling.as_bytes()),
                        "{path}: {name} vs {bytes:?}"
                    );
                }
            }
        }
    }
}

#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc",
    feature = "rust",
    feature = "java",
    feature = "lua",
    feature = "javascript",
    feature = "mozjs",
    feature = "groovy",
    feature = "csharp",
    feature = "php",
    feature = "python",
    feature = "perl",
    feature = "tcl",
    feature = "irules"
))]
#[test]
#[allow(clippy::too_many_lines)]
fn promised_form_fixtures_match_their_expected_outcomes() {
    use big_code_analysis_ast::documentation::{CandidateDisposition, CollectionError};
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/documentation");
    let forms: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("forms.json")).unwrap()).unwrap();
    let rows = forms["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    let mut checked = 0;
    for row in rows {
        let paths: Vec<&str> = if let Some(paths) = row.get("paths") {
            paths
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap())
                .collect()
        } else {
            vec![row["path"].as_str().unwrap()]
        };
        for path in paths {
            let bytes = std::fs::read(directory.join("sources").join(path)).unwrap();
            let languages = if row.get("paths").is_some() {
                let ext = std::path::Path::new(path)
                    .extension()
                    .unwrap()
                    .to_str()
                    .unwrap();
                vec![match ext {
                    "tcl" => "tcl".parse::<LANG>().unwrap(),
                    "irule" => "irules".parse::<LANG>().unwrap(),
                    other => panic!("unknown form extension {other}"),
                }]
            } else {
                form_languages(row)
            };
            for language in languages.into_iter().filter(LANG::is_enabled) {
                let ast = Ast::parse(Source::new(language, &bytes)).unwrap();
                let options = DocumentationOptions {
                    retain_unbound: true,
                    ..Default::default()
                };
                let result = ast.documentation(&DocumentationContext::default(), &options);
                let unavailable = match path.rsplit('.').next().unwrap() {
                    "py" => Some(Profile::Python),
                    "irule" => Some(Profile::Irules),
                    _ => None,
                };
                if let Some(profile) = unavailable {
                    assert!(
                        matches!(result, Err(CollectionError::AdapterUnavailable {
                        language: got, profile: found
                    }) if got == language && found == profile),
                        "{path}"
                    );
                    checked += 1;
                    continue;
                }
                let index = result.unwrap();
                if path == "php-define.php" {
                    let alternate = DocumentationOptions {
                        profile: Profile::PhpDocumentor,
                        ..Default::default()
                    };
                    assert_eq!(
                        ast.documentation(&DocumentationContext::default(), &alternate)
                            .err(),
                        Some(CollectionError::AdapterUnavailable {
                            language,
                            profile: Profile::PhpDocumentor,
                        }),
                        "{path}: promised alternate profile is pending"
                    );
                }
                let promised = row
                    .get("profiles")
                    .map_or(&row["documents"], |v| &v["BcaPhpSource"]);
                let promised = promised.as_array().unwrap();
                match path {
                    // These promises are pending or differ at the evidence base. Keep the
                    // distinction visible rather than treating missing docs as conformance.
                    "perl-mixed.pl" => {
                        assert_eq!(index.profile(), Profile::Pod, "{path}");
                        let documents: Vec<_> = index.documents().collect();
                        let [document] = documents.as_slice() else {
                            panic!("{path}: only the mixed POD block is selected")
                        };
                        let fragment = index.facts().document_fragment(*document, 0).unwrap();
                        let [piece] = fragment.get().source().pieces() else {
                            panic!("{path}: one mixed POD fragment")
                        };
                        assert_eq!(piece.range(), 0..62, "{path}");
                        assert!(index.complete_text(*document).unwrap().is_none(), "{path}");
                        let associations: Vec<_> = index.associations().collect();
                        let [association] = associations.as_slice() else {
                            panic!("{path}: one file association")
                        };
                        assert_eq!(association.get().resolution, Resolution::Resolved, "{path}");
                        assert_eq!(association.get().owner_count(), 1, "{path}");
                        assert!(matches!(
                            index
                                .facts()
                                .associated_owner(*association, 0)
                                .unwrap()
                                .get(),
                            Owner::File(_)
                        ));
                        let assessments: Vec<_> = index.assessments().collect();
                        assert_eq!(assessments.len(), 2, "{path}");
                        assert!(assessments.iter().any(|assessment| {
                            assessment.get().axis == big_code_analysis_ast::documentation::AssessmentAxis::Composition
                                && assessment.get().node.is_some_and(|node| node.node().byte_range() == (0..62))
                                && index.assessed_document(*assessment).unwrap() == Some(*document)
                        }), "{path}: mixed POD composition is pending");
                        assert!(assessments.iter().any(|assessment| {
                            assessment.get().axis == big_code_analysis_ast::documentation::AssessmentAxis::Occurrence
                                && assessment.get().node.is_some_and(|node| node.node().byte_range() == (91..121))
                                && index.assessed_document(*assessment).unwrap().is_none()
                        }), "{path}: data section occurrence is pending");
                    }
                    "php-define.php" => {
                        assert!(
                            !index.assessments().collect::<Vec<_>>().is_empty(),
                            "{path}"
                        );
                        assert_eq!(index.documents().len(), promised.len(), "{path}");
                        for (document, expected) in index.documents().zip(promised) {
                            assert_eq!(
                                index.complete_text(document).unwrap().unwrap().text(),
                                expected["text"].as_str().unwrap(),
                                "{path}"
                            );
                        }
                    }
                    "groovy-spaces.groovy" => {
                        assert_eq!(index.assessments().len(), 0, "{path}");
                        check_form_documents(&index, &bytes, promised, path);
                    }
                    "rust-reach.rs" => {
                        check_form_documents(&index, &bytes, promised, path);
                        assert!(index.assessments().len() > 0, "{path}");
                    }
                    "doxygen-quoted.h" => {
                        assert_eq!(index.documents().len(), 2, "{path}");
                        let texts: Vec<_> = index
                            .documents()
                            .map(|d| index.complete_text(d).unwrap().unwrap().text().to_owned())
                            .collect();
                        assert_eq!(
                            texts,
                            [
                                "\"@file\"\n<![CDATA[ @file ]]>\nfirst\nsecond",
                                "\"x\" @file"
                            ],
                            "{path}"
                        );
                    }
                    _ => check_form_documents(&index, &bytes, promised, path),
                }
                let selected: Vec<_> = index
                    .evidence()
                    .filter(|candidate| {
                        candidate.get().disposition == CandidateDisposition::Superseded
                    })
                    .map(|candidate| {
                        let fragment = index.evidence_fragment(candidate).unwrap();
                        let [piece] = fragment.get().source().pieces() else {
                            panic!("{path}: expected one retained superseded spelling")
                        };
                        let span = piece.range();
                        [span.start, span.end]
                    })
                    .collect();
                assert_eq!(
                    serde_json::json!(selected),
                    row.get("superseded")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                    "{path}"
                );
                if !row
                    .get("evidence")
                    .is_some_and(serde_json::Value::is_string)
                {
                    let evidence = row.get("evidence").and_then(serde_json::Value::as_array);
                    let candidates: Vec<_> = index.evidence().collect();
                    let unattached: Vec<_> = candidates
                        .into_iter()
                        .filter(|candidate| {
                            candidate.get().disposition == CandidateDisposition::Unattached
                        })
                        .collect();
                    assert_eq!(unattached.len(), evidence.map_or(0, Vec::len), "{path}");
                    for (candidate, expected) in
                        unattached.into_iter().zip(evidence.into_iter().flatten())
                    {
                        assert_eq!(
                            candidate.get().disposition,
                            CandidateDisposition::Unattached,
                            "{path}"
                        );
                        let fragment = index.evidence_fragment(candidate).unwrap();
                        let pieces = fragment.get().source().pieces();
                        assert_eq!(pieces.len(), 1, "{path}");
                        let span = pieces[0].range();
                        assert!(span.end <= bytes.len(), "{path}");
                        if let Some(range) = expected.get("fragments") {
                            assert_eq!(
                                serde_json::json!([[span.start, span.end]]),
                                *range,
                                "{path}"
                            );
                        } else {
                            assert_eq!(
                                &bytes[span],
                                expected["fragment_spelling"].as_str().unwrap().as_bytes(),
                                "{path}"
                            );
                        }
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "javascript",
    feature = "php",
    feature = "rust",
    feature = "typescript",
    feature = "go"
))]
#[test]
#[allow(clippy::too_many_lines)]
fn representative_corpus_files_collect_valid_documentation_and_preserve_metrics() {
    use big_code_analysis::{Metric, MetricsOptions, analyze, guess_language};
    use big_code_analysis_ast::documentation::{CollectionError, Origin, OriginView, SourcePiece};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/repositories");
    let mut examined = 0;
    let mut selected_all = Vec::new();
    for corpus in ["serde", "pdf.js", "DeepSpeech", "big-code-analysis-output"] {
        let directory = root.join(corpus);
        if !directory.is_dir() || std::fs::read_dir(&directory).unwrap().next().is_none() {
            continue;
        }
        let mut enabled_candidates = 0;
        let mut selected = Vec::new();
        for entry in walkdir::WalkDir::new(&directory)
            .max_depth(8)
            .sort_by_file_name()
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".git")
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !entry.metadata().is_ok_and(|m| m.len() <= 131_072) {
                continue;
            }
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some(
                    "rs" | "js"
                        | "ts"
                        | "go"
                        | "java"
                        | "py"
                        | "c"
                        | "cc"
                        | "cpp"
                        | "h"
                        | "hh"
                        | "php"
                        | "rb"
                        | "cs"
                        | "lua"
                        | "groovy"
                        | "m"
                )
            ) {
                continue;
            }
            let Ok(Some(bytes)) = big_code_analysis::read_file_with_eol(path) else {
                continue;
            };
            if !bytes
                .windows(3)
                .any(|v| matches!(v, b"///" | b"/**" | b"--["))
            {
                continue;
            }
            let Some(language) = guess_language(&bytes, path).0 else {
                continue;
            };
            if !language.is_enabled() {
                continue;
            }
            enabled_candidates += 1;
            selected.push(path.to_path_buf());
            if selected.len() == 3 {
                break;
            }
        }
        if enabled_candidates > 0 {
            assert!(
                !selected.is_empty(),
                "no supported source in {}",
                directory.display()
            );
        }
        selected_all.extend(selected);
    }
    // An archive has no submodules. Exercise the same assertions on a real in-tree
    // fixture so the metrics guard does not disappear from archive-only sweeps.
    if selected_all.is_empty() {
        let fixture = if cfg!(feature = "rust") {
            "rust-attribute.rs"
        } else if cfg!(feature = "go") {
            "primary.go"
        } else if cfg!(feature = "cpp") {
            "primary.cpp"
        } else if cfg!(feature = "c") {
            "primary.c"
        } else if cfg!(feature = "csharp") {
            "primary.cs"
        } else if cfg!(feature = "javascript") {
            "primary.js"
        } else if cfg!(feature = "php") {
            "primary.php"
        } else {
            "primary.ts"
        };
        selected_all.push(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/documentation/sources")
                .join(fixture),
        );
    }
    for path in selected_all {
        let ast = Ast::from_path(&path).unwrap();
        let metrics = || {
            let space = analyze(
                Source::new(ast.language(), ast.source()),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            (space.metrics.loc.dloc(), space.metrics.loc.cloc())
        };
        let before = metrics();
        let index = match ast.documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        ) {
            Ok(index) => index,
            Err(CollectionError::AdapterUnavailable { .. }) => continue,
            Err(error) => panic!("{}: {error:?}", path.display()),
        };
        assert_eq!(index.source(), ast.source());
        let len = ast.source().len();
        let facts = index.facts();
        for document in index.documents() {
            assert!(document.get().fragment_count() > 0, "{}", path.display());
            for ordinal in 0..document.get().fragment_count() {
                let fragment = facts.document_fragment(document, ordinal).unwrap();
                for piece in fragment.get().source().pieces() {
                    let span = piece.range();
                    assert!(
                        span.start < span.end && span.end <= len,
                        "{}",
                        path.display()
                    );
                }
                if let Some(node) = fragment.get().node() {
                    assert!(
                        node.node().start_byte() <= node.node().end_byte()
                            && node.node().end_byte() <= len,
                        "{}",
                        path.display()
                    );
                }
            }
            if let Some(complete) = index.complete_text(document).unwrap() {
                let mut end = 0;
                for mapping in complete.mappings() {
                    let output = mapping.output.range();
                    assert_eq!(output.start, end, "{}", path.display());
                    end = output.end;
                    let pieces = match &mapping.origin {
                        Origin::Copied(p) | Origin::Decoded(p) | Origin::Expanded(p) => {
                            Some(p.pieces())
                        }
                        Origin::Synthetic => None,
                        other => panic!("unexpected origin {other:?}"),
                    };
                    for piece in pieces.into_iter().flatten() {
                        let span = piece.range();
                        assert!(
                            span.start < span.end && span.end <= len,
                            "{}",
                            path.display()
                        );
                    }
                }
                assert_eq!(end, complete.text().len(), "{}", path.display());
                for portion in complete.origins(0..complete.text().len()).unwrap() {
                    assert!(
                        portion.output.start <= portion.output.end
                            && portion.output.end <= complete.text().len(),
                        "{}",
                        path.display()
                    );
                    let spans: Option<Vec<_>> = match portion.origin {
                        OriginView::Copied(piece) => Some(vec![piece.range()]),
                        OriginView::Decoded(pieces) | OriginView::Expanded(pieces) => {
                            Some(pieces.iter().map(SourcePiece::range).collect())
                        }
                        OriginView::Synthetic => None,
                        other => panic!("unexpected origin view {other:?}"),
                    };
                    for span in spans.into_iter().flatten() {
                        assert!(
                            span.start < span.end && span.end <= len,
                            "{}",
                            path.display()
                        );
                    }
                }
            }
        }
        for association in index.associations() {
            let scope = association.get().lexical_scope.node().byte_range();
            assert!(
                scope.start <= scope.end && scope.end <= len,
                "{}",
                path.display()
            );
            if let Some(binding) = association.get().binding {
                let span = binding.node().byte_range();
                assert!(
                    span.start <= span.end && span.end <= len,
                    "{}",
                    path.display()
                );
            }
            for ordinal in 0..association.get().owner_count() {
                let owner = facts.associated_owner(association, ordinal).unwrap();
                match owner.get() {
                    Owner::Node(n) | Owner::File(n) => {
                        let span = n.node().byte_range();
                        assert!(
                            span.start <= span.end && span.end <= len,
                            "{}",
                            path.display()
                        );
                    }
                    Owner::Slot(s) => {
                        for piece in s.source.pieces() {
                            assert!(piece.range().end <= len, "{}", path.display());
                        }
                    }
                    _ => {}
                }
            }
        }
        let after = metrics();
        assert_eq!(before, after, "{}", path.display());
        examined += 1;
    }
    assert!(examined > 0);
}
