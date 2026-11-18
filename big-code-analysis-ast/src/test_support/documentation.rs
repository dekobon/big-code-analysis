//! Independent documentation requirements and raw fixture routing.
//!
//! These describe required source extraction, not current adapter capabilities.
//! Current recognition is tested separately against the production Checker.

use crate::LANG;

/// Independent primary expectations, including legacy syntax observations.
///
/// # Panics
/// If the checked inventory is not valid JSON.
#[must_use]
pub fn inventory() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/inventory.json"
    ))
    .expect("independent documentation inventory")
}

/// Compare a future adapter's primary result with the independent requirement.
///
/// The adapter projects its result to ordered records with `fragments`, `owner`
/// and `text` keys. Helpers must report `NoDeclarationModel`. Disabled languages
/// are checked separately by the adapter's feature tests, never as positives.
/// This harness does not call or claim an implemented extraction API.
///
/// # Panics
/// If an enabled source language returns an error, empty documents, wrong ranges,
/// incomplete text, or any result differing from the primary fixture.
pub fn assert_primary_extraction(lang: LANG, result: Result<serde_json::Value, &str>) {
    assert!(
        lang.is_enabled(),
        "a disabled language cannot qualify as a positive"
    );
    let expected = &inventory()[lang.to_string()];
    match requirement(lang) {
        Requirement::DeclarationText => {
            assert_eq!(
                result.expect("source adapter must produce declaration text"),
                serde_json::json!([{
                    "fragments": expected["fragments"], "owner": expected["owner"], "text": expected["text"]
                }])
            );
        }
        Requirement::NoDeclarationModel => assert_eq!(result, Err("NoDeclarationModel")),
    }
}

/// Required outcome for a language, even when its adapter is not implemented.
#[derive(Debug, PartialEq, Eq)]
pub enum Requirement {
    /// A declaration-associated document with nonempty local text is required.
    DeclarationText,
    /// Helper grammars have no declaration model.
    NoDeclarationModel,
}

/// Exhaustive policy independent of runtime capability reporting.
#[must_use]
pub const fn requirement(lang: LANG) -> Requirement {
    match lang {
        LANG::Bash
        | LANG::C
        | LANG::Cpp
        | LANG::Csharp
        | LANG::Elixir
        | LANG::Go
        | LANG::Groovy
        | LANG::Irules
        | LANG::Java
        | LANG::Javascript
        | LANG::Kotlin
        | LANG::Lua
        | LANG::Mozcpp
        | LANG::Mozjs
        | LANG::Objc
        | LANG::Perl
        | LANG::Php
        | LANG::Python
        | LANG::Ruby
        | LANG::Rust
        | LANG::Tcl
        | LANG::Tsx
        | LANG::Typescript => Requirement::DeclarationText,
        LANG::Ccomment | LANG::Preproc => Requirement::NoDeclarationModel,
    }
}

/// Raw source for a named fixture. Never uses the normalizing file loader.
///
/// # Panics
/// If the tracked fixture is missing or unreadable.
#[must_use]
pub fn source(path: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/documentation/sources")
            .join(path),
    )
    .expect("tracked raw documentation fixture")
}

/// One primary input per LANG variant, including explicit helper controls.
#[must_use]
pub const fn primary(lang: LANG) -> &'static str {
    match lang {
        LANG::Bash => "primary.bash",
        LANG::C => "primary.c",
        LANG::Ccomment => "helper-comment.c",
        LANG::Cpp | LANG::Mozcpp => "primary.cpp",
        LANG::Csharp => "primary.cs",
        LANG::Elixir => "primary.ex",
        LANG::Go => "primary.go",
        LANG::Groovy => "primary.groovy",
        LANG::Irules => "primary.irule",
        LANG::Java => "primary.java",
        LANG::Javascript | LANG::Mozjs => "primary.js",
        LANG::Kotlin => "primary.kt",
        LANG::Lua => "primary.lua",
        LANG::Objc => "primary.m",
        LANG::Perl => "primary.pl",
        LANG::Php => "primary.php",
        LANG::Preproc => "helper-preproc.h",
        LANG::Python => "primary.py",
        LANG::Ruby => "primary.rb",
        LANG::Rust => "primary.rs",
        LANG::Tcl => "primary.tcl",
        LANG::Tsx => "primary.tsx",
        LANG::Typescript => "primary.ts",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Checker, test_support::for_each_node_with_sibling_cache_tolerating_errors};

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
    fn documentation_primary_recognition() {
        let inventory = inventory();
        let mut ran = 0;
        assert_eq!(
            inventory.as_object().expect("LANG inventory").len(),
            LANG::into_enum_iter().count()
        );
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let entry = &inventory[lang.to_string()];
            let source = source(primary(lang));
            let mut comments = Vec::new();
            let mut owners = Vec::new();
            crate::with_code_tag!(lang, |C| {
                let (_, recovered) =
                    for_each_node_with_sibling_cache_tolerating_errors::<C>(&source, |node, _| {
                        if C::is_comment(node) {
                            comments.push(serde_json::json!([
                                node.start_byte(),
                                node.end_byte(),
                                C::is_doc_comment(node, &source)
                            ]));
                        }
                        if node.is_named()
                            && serde_json::json!([node.start_byte(), node.end_byte()])
                                == entry["owner"]
                        {
                            owners.push(node.kind());
                        }
                    });
                assert_eq!(
                    recovered,
                    entry["recovered"] == true,
                    "{lang}: parse recovery"
                );
            });
            assert_eq!(
                serde_json::json!(comments),
                entry["legacy_comments"],
                "{lang}"
            );
            match requirement(lang) {
                Requirement::DeclarationText => {
                    assert!(
                        !entry["text"]
                            .as_str()
                            .expect("required local text")
                            .is_empty(),
                        "{lang}"
                    );
                    assert_eq!(entry["fragments"].as_array().expect("fragments").len(), 1);
                    assert_eq!(
                        serde_json::json!(owners),
                        entry["owner_kinds"],
                        "{lang}: declaration syntax"
                    );
                    assert!(!owners.is_empty(), "missing declaration for {lang}");
                }
                Requirement::NoDeclarationModel => {
                    assert_eq!(entry["profile"], "NoDeclarationModel");
                    assert!(entry["owner"].is_null());
                }
            }
            ran += 1;
        }
        assert!(ran > 0);
    }

    #[cfg(feature = "rust")]
    #[test]
    fn documentation_conformance_rejects_absent_adapter_and_wrong_identity() {
        for result in [
            Err("Unsupported"),
            Err("NoDeclarationModel"),
            Ok(serde_json::json!([])),
            Ok(serde_json::json!([{"fragments":[[0,8]],"owner":[9,18],"text":""}])),
            Ok(serde_json::json!([{"fragments":[[1,9]],"owner":[9,18],"text":"audit"}])),
            Ok(serde_json::json!([{"fragments":[[0,8]],"owner":[0,18],"text":"audit"}])),
        ] {
            assert!(
                std::panic::catch_unwind(|| assert_primary_extraction(LANG::Rust, result)).is_err()
            );
        }
    }

    #[cfg(feature = "java")]
    #[test]
    fn documentation_record_header_and_compact_constructor_are_distinct_nodes() {
        for (file, expected) in [
            ("java-record.java", vec![("formal_parameters", 26, 33)]),
            (
                "java-compact.java",
                vec![("compact_constructor_declaration", 36, 40)],
            ),
        ] {
            let source = source(file);
            let mut callables = Vec::new();
            super::super::for_each_node_with_sibling_cache::<crate::JavaCode>(
                &source,
                |node, ancestors| {
                    if crate::JavaCode::is_func_with_code(node, &source, ancestors) {
                        callables.push((node.kind(), node.start_byte(), node.end_byte()));
                    }
                },
            );
            assert_eq!(callables, expected, "{file}");
        }
    }

    #[cfg(feature = "python")]
    #[test]
    fn documentation_named_continuation_extras_remain_in_the_tree() {
        let source = source("python-extras.py");
        let mut extras = Vec::new();
        super::super::for_each_node_with_sibling_cache::<crate::PythonCode>(&source, |node, _| {
            if node.kind() == "line_continuation" {
                extras.push((node.start_byte(), node.end_byte(), node.is_named()));
            }
        });
        assert_eq!(extras, [(15, 17, true), (21, 23, true)]);
    }
}
