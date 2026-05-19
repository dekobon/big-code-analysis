// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::path::Path;
use std::sync::Arc;
use tree_sitter::Language;

// `get_language` is referenced from feature-gated arms inside the
// `mk_lang!` expansion; an `--no-default-features` build with no
// language features compiles every arm out, leaving the import
// nominally unused. The macro itself carries the same allow.
#[allow(unused_imports)]
use crate::macros::{
    get_language, mk_action, mk_code, mk_emacs_mode, mk_extensions, mk_lang, mk_langs,
};
use crate::preproc::PreprocResults;
use crate::*;

mk_langs!(
    // 1) Cargo feature name that enables this variant's grammar
    // 2) Name for enum
    // 3) Language description
    // 4) Display name
    // 5) Empty struct name to implement
    // 6) Parser name
    // 7) tree-sitter function to call to get a Language
    // 8) file extensions
    // 9) emacs modes
    //
    // Per #252, each variant carries a Cargo feature that gates the
    // grammar crate references in `mk_lang!` / `mk_action!`. The enum
    // surface (variants, file-extension lookup, emacs-mode lookup,
    // per-language `*Code` / `*Parser` tags) is always compiled in;
    // disabling a feature only strips the grammar crate from the dep
    // graph and turns every dispatcher into
    // `Err(MetricsError::LanguageDisabled(_))`.
    //
    // `Ccomment` and `Preproc` ride the `cpp` feature because they
    // are internal helpers for the C/C++ pipeline; they share the
    // `tree-sitter-ccomment` / `tree-sitter-preproc` crates that
    // `cpp` (and `mozcpp`) pull in. `Tsx` rides `typescript` because
    // both variants resolve to the `tree-sitter-typescript` crate
    // (TSX vs TypeScript is a per-grammar `LANGUAGE_*` constant
    // inside that one crate, see `get_language!` in `src/macros.rs`).
    (
        "mozjs",
        Mozjs,
        "The `Mozjs` language is variant of the `JavaScript` language",
        "javascript",
        MozjsCode,
        MozjsParser,
        tree_sitter_mozjs,
        [js, jsm, mjs, jsx],
        ["js", "js2"]
    ),
    (
        "javascript",
        Javascript,
        "The `JavaScript` language",
        "javascript",
        JavascriptCode,
        JavascriptParser,
        tree_sitter_javascript,
        [],
        []
    ),
    (
        "java",
        Java,
        "The `Java` language",
        "java",
        JavaCode,
        JavaParser,
        tree_sitter_java,
        [java],
        ["java"]
    ),
    (
        "go",
        Go,
        "The `Go` language",
        "go",
        GoCode,
        GoParser,
        tree_sitter_go,
        [go],
        ["go"]
    ),
    (
        "kotlin",
        Kotlin,
        "The `Kotlin` language",
        "kotlin",
        KotlinCode,
        KotlinParser,
        tree_sitter_kotlin_ng,
        [kt, kts],
        ["kotlin"]
    ),
    (
        "lua",
        Lua,
        "The `Lua` language",
        "lua",
        LuaCode,
        LuaParser,
        tree_sitter_lua,
        [lua],
        ["lua"]
    ),
    (
        "rust",
        Rust,
        "The `Rust` language",
        "rust",
        RustCode,
        RustParser,
        tree_sitter_rust,
        [rs],
        ["rust"]
    ),
    (
        "tcl",
        Tcl,
        "The `Tcl` language",
        "tcl",
        TclCode,
        TclParser,
        tree_sitter_tcl,
        [tcl, tk, tm],
        ["tcl"]
    ),
    (
        "cpp",
        Cpp,
        "The `C/C++` language",
        "c/c++",
        CppCode,
        CppParser,
        tree_sitter_cpp,
        [cpp, cxx, cc, hxx, hpp, c, h, hh, inc, mm, m],
        ["c++", "c", "objc", "objc++", "objective-c++", "objective-c"]
    ),
    (
        "csharp",
        Csharp,
        "The `C#` language",
        "c#",
        CsharpCode,
        CsharpParser,
        tree_sitter_c_sharp,
        [cs, csx, cake],
        ["csharp"]
    ),
    (
        "elixir",
        Elixir,
        "The `Elixir` language",
        "elixir",
        ElixirCode,
        ElixirParser,
        tree_sitter_elixir,
        [ex, exs],
        ["elixir"]
    ),
    (
        "python",
        Python,
        "The `Python` language",
        "python",
        PythonCode,
        PythonParser,
        tree_sitter_python,
        [py],
        ["python"]
    ),
    (
        "typescript",
        Tsx,
        "The `Tsx` language incorporates the `JSX` syntax inside `TypeScript`",
        "typescript",
        TsxCode,
        TsxParser,
        tree_sitter_tsx,
        [tsx],
        []
    ),
    (
        "typescript",
        Typescript,
        "The `TypeScript` language",
        "typescript",
        TypescriptCode,
        TypescriptParser,
        tree_sitter_typescript,
        [ts, jsw, jsmw],
        ["typescript"]
    ),
    (
        "bash",
        Bash,
        "The `Bash` language",
        "bash",
        BashCode,
        BashParser,
        tree_sitter_bash,
        [sh, bash],
        ["sh"]
    ),
    (
        "cpp",
        Ccomment,
        "The `Ccomment` language is a variant of the `C` language focused on comments",
        "ccomment",
        CcommentCode,
        CcommentParser,
        tree_sitter_ccomment,
        [],
        []
    ),
    (
        "cpp",
        Preproc,
        "The `PreProc` language is a variant of the `C/C++` language focused on macros",
        "preproc",
        PreprocCode,
        PreprocParser,
        tree_sitter_preproc,
        [],
        []
    ),
    (
        "perl",
        Perl,
        "The `Perl` language",
        "perl",
        PerlCode,
        PerlParser,
        tree_sitter_perl,
        [pl, pm, t],
        ["perl", "cperl"]
    ),
    (
        "php",
        Php,
        "The `Php` language",
        "php",
        PhpCode,
        PhpParser,
        tree_sitter_php,
        [php, phtml, php3, php4, php5, php7, phps],
        ["php"]
    ),
    (
        "ruby",
        Ruby,
        "The `Ruby` language",
        "ruby",
        RubyCode,
        RubyParser,
        tree_sitter_ruby,
        [rb, rake, gemspec],
        ["ruby"]
    ),
    (
        "groovy",
        Groovy,
        "The `Groovy` language",
        "groovy",
        GroovyCode,
        GroovyParser,
        tree_sitter_groovy,
        [groovy, gradle, gvy, gy, gsh],
        ["groovy"]
    )
);

pub(crate) mod fake {
    pub(crate) fn get_true<'a>(ext: &str, mode: &str) -> Option<&'a str> {
        if ext == "m"
            || ext == "mm"
            || mode == "objc"
            || mode == "objc++"
            || mode == "objective-c++"
            || mode == "objective-c"
        {
            Some("obj-c/c++")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MetricsError;

    // The test suite normally runs under the workspace default
    // feature set (`all-languages` is on, see `Cargo.toml`), so
    // every variant must report itself as enabled. A regression in
    // the cfg-gating of `is_enabled` would flip individual arms to
    // `false` even when the matching grammar crate is in the dep
    // graph; this test would catch that without needing a separate
    // `--no-default-features` build matrix entry. Gated on
    // `feature = "all-languages"` so the CI minimal-langs matrix
    // entry (`--no-default-features --features rust,typescript`)
    // still compiles cleanly without a runtime failure.
    #[cfg(feature = "all-languages")]
    #[test]
    fn every_lang_variant_is_enabled_under_all_languages() {
        for lang in LANG::into_enum_iter() {
            assert!(
                lang.is_enabled(),
                "{} should be enabled under the default `all-languages` feature set",
                lang.get_name(),
            );
        }
    }

    // Smoke test for the `LanguageDisabled` contract on a build
    // without the `javascript` feature: every dispatch entry point
    // (here, `get_tree_sitter_language`) must hand back
    // `Err(LanguageDisabled(LANG::Javascript))`. Gated on
    // `not(feature = "javascript")` so it only runs in a feature-
    // subset build where the language is actually disabled — the
    // `all-languages` default would have `is_enabled` return true
    // and `get_tree_sitter_language` succeed.
    #[cfg(not(feature = "javascript"))]
    #[test]
    fn disabled_language_dispatch_returns_language_disabled() {
        assert!(!LANG::Javascript.is_enabled());
        match LANG::Javascript.get_tree_sitter_language() {
            Err(MetricsError::LanguageDisabled(LANG::Javascript)) => {}
            other => panic!(
                "expected Err(LanguageDisabled(Javascript)) for disabled `javascript` feature, got {other:?}",
            ),
        }
    }

    // `is_enabled` and `get_tree_sitter_language` must agree: a
    // variant that reports itself enabled must hand back a usable
    // `Language`, never `Err(LanguageDisabled)`. The pairing exists
    // so callers that branch on `is_enabled` (rather than match on
    // the error) can rely on the language lookup succeeding.
    #[test]
    fn is_enabled_matches_get_tree_sitter_language() {
        for lang in LANG::into_enum_iter() {
            let lookup = lang.get_tree_sitter_language();
            assert_eq!(
                lang.is_enabled(),
                lookup.is_ok(),
                "{} disagrees: is_enabled={}, get_tree_sitter_language={:?}",
                lang.get_name(),
                lang.is_enabled(),
                lookup.map(|_| "Ok"),
            );
        }
    }

    // Regression guard for issue #262: the `MetricsError::EmptyRoot`
    // variant is documented as "Reserved — not produced today".
    // `metrics_with_options` pushes a synthetic top-level Unit
    // `FuncSpace` before walking, so every parse — including empty,
    // whitespace-only, and comment-only input — currently returns
    // `Ok(FuncSpace { kind: Unit, .. })`. If the walker is ever
    // changed to legitimately drain its state stack (e.g. by
    // dropping the synthetic root), this test will start failing
    // and the variant docs must be revisited.
    #[test]
    fn empty_and_comment_only_input_never_returns_empty_root() {
        use crate::{MetricsOptions, Source, SpaceKind, analyze};

        // Pair every enabled language with sources that would, by
        // the old (false) variant doc, surface `EmptyRoot`. The
        // comment syntaxes cover line and block forms across the
        // supported language families.
        let inputs: &[&[u8]] = &[b"", b"   \n\t\n", b"// just a comment\n", b"/* block */\n"];

        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            for src in inputs {
                let space = analyze(Source::new(lang, src), MetricsOptions::default())
                    .unwrap_or_else(|err| {
                        panic!(
                            "{} on input {:?} unexpectedly returned {err:?}; \
                             EmptyRoot is documented as not produced today",
                            lang.get_name(),
                            String::from_utf8_lossy(src),
                        )
                    });
                assert_eq!(
                    space.kind,
                    SpaceKind::Unit,
                    "{} on input {:?} produced a non-Unit top-level FuncSpace",
                    lang.get_name(),
                    String::from_utf8_lossy(src),
                );
            }
        }
    }

    // The error variant carries the originating `LANG` so callers
    // can distinguish "X is disabled" from "Y is disabled" in a
    // mixed batch. Verifies the `Display` impl mentions the
    // language name as documented in `src/error.rs`.
    #[test]
    fn language_disabled_display_includes_language_name() {
        let err = MetricsError::LanguageDisabled(LANG::Rust);
        let rendered = err.to_string();
        assert!(
            rendered.contains("rust"),
            "expected LanguageDisabled display to mention `rust`, got {rendered:?}",
        );
    }
}
