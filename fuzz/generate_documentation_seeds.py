#!/usr/bin/env python3
"""Regenerate the byte-exact documentation corpus and its external registry.

Run from any directory. Seed expectations come from the raw fixture inventory
or the explicitly named production regression, never from a collector run.
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FIXTURES = ROOT.parent / "tests/fixtures/documentation"
LANGUAGES = [
    "rust",
    "python",
    "java",
    "kotlin",
    "groovy",
    "csharp",
    "php",
    "javascript",
    "mozjs",
    "typescript",
    "tsx",
    "c",
    "cpp",
    "mozcpp",
    "objc",
    "go",
    "ruby",
    "lua",
    "perl",
    "elixir",
    "bash",
    "tcl",
    "irules",
    "ccomment",
    "preproc",
]
DEFERRED = {"python", "perl", "elixir", "bash", "tcl", "irules"}


def main():
    """Write deliberate seed bytes; reject unregistered pre-existing files."""
    inventory = json.loads((FIXTURES / "inventory.json").read_text())
    records = []
    seeds = {}

    def add(name, header, payload, assertion, provenance, **expected):
        if ".rs" in provenance and not provenance.startswith("tests/"):
            prefix = (
                "" if provenance.startswith(("text/", "streaming/")) else "collector/"
            )
            provenance = (
                "big-code-analysis-ast/src/documentation/" + prefix + provenance
            )
        seeds[name] = bytes(header) + payload
        records.append(
            {
                "name": name,
                "header": header,
                "sha256": hashlib.sha256(payload).hexdigest(),
                "class": assertion,
                "provenance": provenance,
                **expected,
            }
        )

    for tag, language in enumerate(LANGUAGES):
        row = inventory[language]
        assertion = (
            "deferred"
            if language in DEFERRED
            else ("helper" if language in {"ccomment", "preproc"} else "positive")
        )
        add(
            f"dispatch-{language}",
            [tag, 1, 0],
            (FIXTURES / "sources" / row["path"]).read_bytes(),
            assertion,
            "tests/fixtures/documentation/inventory.json",
            inventory=language,
            fixture=row["path"],
            has_error=language in {"tcl", "irules"},
        )

    for name, fixture, assertion in [
        ("rust-eof", "rust-eof.rs", "eof"),
        ("rust-crlf", "rust-crlf.rs", "crlf"),
        ("rust-attribute", "rust-attribute.rs", "rust-literal-mixed"),
    ]:
        add(
            name,
            [0, 1, 0],
            (FIXTURES / "sources" / fixture).read_bytes(),
            assertion,
            "tests/fixtures/documentation/forms.json",
            fixture=fixture,
            has_error=False,
        )

    cases = [
        ("rust-literal-empty", [0, 3, 0], b'#[doc = ""] fn f() {}', "rust-literal-empty", "rust_tests.rs::rust_literal_cleanup_pins_mixed_margin_empty_joins_and_trailing_spaces", False),
        ("rust-literal-raw", [0, 3, 0], b'#[doc = r###"a"#b"###] fn f() {}', "rust-literal-raw", "rust_tests.rs::rust_literal_cleanup_pins_mixed_margin_empty_joins_and_trailing_spaces", False),
        ("rust-literal-scalar", [0, 7, 0], b'#[doc = "\\u{1F600}"] fn f() {}', "rust-literal-scalar", "rust_tests.rs::rust_literal_escape_origins_are_complete_tokens_after_cleanup", False),
        ("rust-literal-continuation", [0, 3, 0], b'#[doc = "a\\\r\n \t\nb"] fn f() {}', "rust-literal-continuation", "rust_tests.rs::rust_literal_escape_origins_are_complete_tokens_after_cleanup", False),
        ("rust-literal-margin", [0, 3, 0], b'///     hello\n#[doc = "another"]\nfn f() {}', "rust-literal-margin", "rust_tests.rs::rust_literal_cleanup_pins_mixed_margin_empty_joins_and_trailing_spaces", False),
        ("rust-pending-metadata", [0, 3, 0], b'#[doc(hidden)] fn f() {}', "pending-content", "tests.rs::rust_pending_attributes_never_report_complete_partial_text", False),
        (
            "rust-join",
            [0, 3, 0],
            b"///a\n///b\nfn f() {}",
            "join",
            "tests.rs::rust_joint_smoke_pins_exact_composition_and_maps",
            False,
        ),
        (
            "c-shared-owners",
            [11, 3, 0],
            b"/**audit*/\nint a, b;",
            "shared",
            "tests.rs::c_declaration_fanout_and_trailing_projection_share_retained_text",
            False,
        ),
        (
            "cpp-gathered",
            [12, 1, 0],
            b"/**a*/\n/**b*/\nvoid f() {}",
            "gathered",
            "remediation_tests.rs::gathered_disposition_does_not_rescan_structural_member_per_fragment",
            False,
        ),
        (
            "cpp-candidate-qualification",
            [12, 3, 0],
            b"#if FLAG\nvoid f(int x = );\n#endif\nvoid f(void);\n/**@fn f*/",
            "candidate-qualification",
            "named_tests.rs::ambiguous_candidates_keep_structured_qualification_and_reference_identity",
            True,
        ),
        (
            "cpp-reference-qualification",
            [12, 3, 0],
            b"void f(void);\n#if FLAG\n/**@fn f*/\n#endif",
            "reference-qualification",
            "named_tests.rs::ambiguous_candidates_keep_structured_qualification_and_reference_identity",
            False,
        ),
        (
            "cpp-missing-name",
            [12, 1, 0],
            b"/**@fn void missing()\n * audit*/\nvoid f();",
            "missing-name",
            "tests.rs::doxygen_static_names_resolve_unique_missing_and_ambiguous_targets",
            False,
        ),
        (
            "cpp-pending-name",
            [12, 1, 0],
            b"void f(int x);\n/**@fn void f(,)*/",
            "pending-name",
            "named_tests.rs::narrow_signature_grammar_defers_malformed_and_unsupported_references",
            False,
        ),
        (
            "kotlin-pending-owner",
            [3, 1, 0],
            b"class C(/**audit*/ val x: Int)",
            "pending-owner",
            "form_tests.rs::kotlin_parameter_form_is_pending_in_default_queries",
            False,
        ),
        (
            "kotlin-known-and-pending",
            [3, 3, 0],
            b"/**first*/\nfun f() /**audit*/ {}",
            "known-and-pending",
            "assessment_tests.rs::kotlin_local_pending_owner_keeps_complete_text_and_known_leading_association",
            False,
        ),
        (
            "rust-invalid-utf8",
            [0, 1, 0],
            b"///\xff\nfn f() {}",
            "invalid-encoding",
            "text/tests.rs invalid comment decoding",
            False,
        ),
        (
            "rust-partial-delimiter",
            [0, 1, 0],
            b"/**",
            "partial",
            "handwritten incomplete block comment",
            True,
        ),
        (
            "java-scalar-pieces",
            [2, 5, 0],
            "/**é*/\nclass C {}".encode(),
            "scalar-pieces",
            "remediation_tests.rs::multiline_text_reuses_retained_fragment_intersections",
            False,
        ),
        (
            "java-crlf-token-pieces",
            [2, 5, 0],
            b"/**a\r\nb*/\nclass C {}",
            "decoded-pieces",
            "text/comments.rs CRLF normalization across included pieces",
            False,
        ),
        (
            "java-source-gap",
            [2, 5, 0],
            b"/**a\x00b*/\nclass C {}",
            "source-gap",
            "handwritten included-range gap through a comment",
            False,
        ),
        (
            "java-empty",
            [2, 1, 0],
            b"/** old */\n/**/\nclass C {}",
            "empty",
            "tests.rs::java_selected_empty_document_keeps_superseded_raw_evidence_only_on_request",
            False,
        ),
        (
            "java-evidence",
            [2, 3, 32],
            b"/** old */\n/**/\nclass C {}",
            "evidence",
            "tests.rs::java_selected_empty_document_keeps_superseded_raw_evidence_only_on_request",
            False,
        ),
        (
            "go-directive-off",
            [15, 1, 0],
            b"//go:generate tool\npackage p",
            "directive-off",
            "streaming/positional/tests.rs::go_directive_only_evidence_is_lazy_and_not_prose",
            False,
        ),
        (
            "go-directive-on",
            [15, 1, 64],
            b"//go:generate tool\npackage p",
            "directive-on",
            "streaming/positional/tests.rs::go_directive_only_evidence_is_lazy_and_not_prose",
            False,
        ),
        (
            "rust-cancelled-scope",
            [0, 8, 0],
            b"///audit\nfn f() {}",
            "cancelled-scope",
            "tests.rs::raw_input_checks_and_cancellation_have_distinct_outcomes",
            False,
        ),
        (
            "rust-active-scope",
            [0, 12, 0],
            b"///audit\nfn f() {}",
            "active-scope",
            "assessment_tests.rs::nested_collection_cancellation_restores_outer_flag_after_unwind",
            False,
        ),
        (
            "rust-incompatible-profile",
            [0, 0, 2],
            b"///audit\nfn f() {}",
            "incompatible-profile",
            "tests.rs::named_profiles_distinguish_deferred_incompatible_and_available",
            False,
        ),
        (
            "ruby-rdoc-deferred",
            [16, 0, 14],
            b"# audit\ndef f\nend",
            "deferred-profile",
            "tests.rs::named_profiles_distinguish_deferred_incompatible_and_available",
            False,
        ),
        (
            "php-documentor-deferred",
            [6, 0, 8],
            b"<?php\n/** audit */\nfunction f() {}",
            "deferred-profile",
            "tests.rs::named_profiles_distinguish_deferred_incompatible_and_available",
            False,
        ),
        (
            "rust-valid-neighbor",
            [0, 1, 0],
            b"fn broken() { let x = 1; }\n///neighbor\nfn neighbor() {}",
            "neighbor",
            "handwritten recovery pair: valid declaration",
            False,
        ),
        (
            "rust-recovery-neighbor",
            [0, 1, 0],
            b"fn broken() { let x = ; }\n///neighbor\nfn neighbor() {}",
            "neighbor",
            "handwritten recovery pair: localized missing expression",
            True,
        ),
    ]
    for name, header, payload, assertion, provenance, has_error in cases:
        add(name, header, payload, assertion, provenance, has_error=has_error)

    corpus = ROOT / "corpus/documentation"
    corpus.mkdir(parents=True, exist_ok=True)
    unknown = {path.name for path in corpus.iterdir()} - seeds.keys()
    if unknown:
        raise SystemExit(f"unregistered corpus files: {sorted(unknown)}")
    for name, data in seeds.items():
        (corpus / name).write_bytes(data)
    (ROOT / "documentation-seeds.json").write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
