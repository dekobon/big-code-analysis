// bca: suppress-file(halstead)
// Per-language enum / table dispatch; file-level halstead is a many-fn
// aggregation artifact, not per-function logic complexity.

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
        "javascript",
        Javascript,
        "The `JavaScript` language (upstream `tree-sitter-javascript` \
         grammar; the default for `.js` / `.mjs` / `.cjs` / `.jsx`)",
        "javascript",
        JavascriptCode,
        JavascriptParser,
        tree_sitter_javascript,
        [js, mjs, cjs, jsx],
        ["js", "js2"]
    ),
    (
        "mozjs",
        Mozjs,
        "The Mozilla/SpiderMonkey `JavaScript` dialect (vendored \
         `tree-sitter-mozjs` fork; opt-in, owns the `.jsm` module \
         extension)",
        "mozjs",
        MozjsCode,
        MozjsParser,
        tree_sitter_mozjs,
        [jsm],
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
        "irules",
        Irules,
        "The `Irules` language",
        "irules",
        IrulesCode,
        IrulesParser,
        tree_sitter_irules,
        [irule, irules],
        ["irules"]
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
        dekobon_tree_sitter_groovy,
        [groovy, gradle, gvy, gy, gsh],
        ["groovy"]
    )
);

pub(crate) mod fake {
    pub(crate) fn get_true(ext: &str, mode: &str) -> Option<&'static str> {
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
                lang.name(),
            );
        }
    }

    // Smoke test for the `LanguageDisabled` contract on a build
    // without the `javascript` feature: every dispatch entry point
    // (here, `tree_sitter_language`) must hand back
    // `Err(LanguageDisabled(LANG::Javascript))`. Gated on
    // `not(feature = "javascript")` so it only runs in a feature-
    // subset build where the language is actually disabled — the
    // `all-languages` default would have `is_enabled` return true
    // and `tree_sitter_language` succeed.
    #[cfg(not(feature = "javascript"))]
    #[test]
    fn disabled_language_dispatch_returns_language_disabled() {
        assert!(!LANG::Javascript.is_enabled());
        match LANG::Javascript.tree_sitter_language() {
            Err(MetricsError::LanguageDisabled(LANG::Javascript)) => {}
            other => panic!(
                "expected Err(LanguageDisabled(Javascript)) for disabled `javascript` feature, got {other:?}",
            ),
        }
    }

    // `is_enabled` and `tree_sitter_language` must agree: a
    // variant that reports itself enabled must hand back a usable
    // `Language`, never `Err(LanguageDisabled)`. The pairing exists
    // so callers that branch on `is_enabled` (rather than match on
    // the error) can rely on the language lookup succeeding.
    #[test]
    fn is_enabled_matches_get_tree_sitter_language() {
        for lang in LANG::into_enum_iter() {
            let lookup = lang.tree_sitter_language();
            assert_eq!(
                lang.is_enabled(),
                lookup.is_ok(),
                "{} disagrees: is_enabled={}, tree_sitter_language={:?}",
                lang.name(),
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
                            lang.name(),
                            String::from_utf8_lossy(src),
                        )
                    });
                assert_eq!(
                    space.kind,
                    SpaceKind::Unit,
                    "{} on input {:?} produced a non-Unit top-level FuncSpace",
                    lang.name(),
                    String::from_utf8_lossy(src),
                );
            }
        }
    }

    // `Display` must agree with `name` for every variant — the
    // impl delegates to it, so this pins that contract against future
    // refactors that might diverge the two.
    #[test]
    fn display_matches_name_for_every_variant() {
        for lang in LANG::into_enum_iter() {
            assert_eq!(lang.to_string(), lang.name());
        }
    }

    // `Display` -> `FromStr` round-trip. The only variants still sharing
    // a display name are `Tsx`/`Typescript` (both "typescript"); the
    // JavaScript pair was split at 2.0 (#507) so `Mozjs` displays "mozjs"
    // and `Javascript` "javascript". Parsing back is therefore not
    // guaranteed to return the *same* variant for the TS pair, but it
    // must return one whose name matches — the round-trip preserves the
    // display name.
    #[test]
    fn display_fromstr_round_trip_preserves_name() {
        use std::str::FromStr;
        for lang in LANG::into_enum_iter() {
            let parsed = LANG::from_str(lang.name())
                .unwrap_or_else(|e| panic!("{} failed to parse back: {e}", lang.name()));
            assert_eq!(
                parsed.name(),
                lang.name(),
                "{} round-tripped to a differently-named variant {:?}",
                lang.name(),
                parsed,
            );
        }
    }

    // The JavaScript pair has distinct display names since #507, so
    // `Display` is injective for it and the round-trip is exact —
    // "javascript" -> `Javascript` (upstream grammar, the default) and
    // "mozjs" -> `Mozjs` (the opt-in Mozilla fork). Pin both so a future
    // reorder or display-string change is deliberate and test-visible.
    #[test]
    fn javascript_pair_has_distinct_names() {
        use std::str::FromStr;
        assert_eq!(LANG::Javascript.name(), "javascript");
        assert_eq!(LANG::Mozjs.name(), "mozjs");
        assert_eq!(LANG::from_str("javascript"), Ok(LANG::Javascript));
        assert_eq!(LANG::from_str("mozjs"), Ok(LANG::Mozjs));
    }

    // The TypeScript pair still shares the "typescript" display name
    // (both ride the upstream `tree-sitter-typescript` crate), so the
    // aliased name resolves to the first variant declared with it.
    #[test]
    fn aliased_typescript_name_resolves_to_first_declared_variant() {
        use std::str::FromStr;
        // Tsx precedes Typescript in `mk_langs!`.
        assert_eq!(LANG::from_str("typescript"), Ok(LANG::Tsx));
    }

    // Extension dispatch after the #507 default-grammar swap: the
    // standard JS extensions resolve to the upstream `Javascript`
    // grammar (including the newly-supported `.cjs`), while the Mozilla
    // fork owns only `.jsm`.
    #[test]
    fn javascript_extension_dispatch_defaults_to_upstream() {
        assert_eq!(get_from_ext("js"), Some(LANG::Javascript));
        assert_eq!(get_from_ext("mjs"), Some(LANG::Javascript));
        assert_eq!(get_from_ext("cjs"), Some(LANG::Javascript));
        assert_eq!(get_from_ext("jsx"), Some(LANG::Javascript));
        assert_eq!(get_from_ext("jsm"), Some(LANG::Mozjs));
    }

    // The `js` / `js2` emacs modes moved to the upstream `Javascript`
    // default alongside the extensions; pin them so a future `mk_langs!`
    // reorder cannot silently reroute emacs-mode dispatch to the fork.
    #[test]
    fn javascript_emacs_mode_dispatch_defaults_to_upstream() {
        assert_eq!(get_from_emacs_mode("js"), Some(LANG::Javascript));
        assert_eq!(get_from_emacs_mode("js2"), Some(LANG::Javascript));
    }

    // The `Cpp` and `Csharp` variants carry punctuation in their
    // display names ("c/c++", "c#"); `FromStr` must accept exactly what
    // `Display` emits, including the punctuation.
    #[test]
    fn punctuated_display_names_round_trip() {
        use std::str::FromStr;
        assert_eq!(LANG::Cpp.to_string(), "c/c++");
        assert_eq!(LANG::from_str("c/c++"), Ok(LANG::Cpp));
        assert_eq!(LANG::Csharp.to_string(), "c#");
        assert_eq!(LANG::from_str("c#"), Ok(LANG::Csharp));
    }

    // Unknown / mis-cased input is rejected; matching is case-sensitive,
    // mirroring `Metric`'s `FromStr`.
    #[test]
    fn fromstr_rejects_unknown_and_miscased() {
        use std::str::FromStr;
        assert!(LANG::from_str("Rust").is_err());
        assert!(LANG::from_str("klingon").is_err());
        assert!(LANG::from_str("").is_err());
        // The error carries the offending input verbatim, recoverable
        // both via `Display` and the additive `input()` accessor (#536).
        let err = LANG::from_str("klingon").unwrap_err();
        assert!(err.to_string().contains("klingon"));
        assert_eq!(err.input(), "klingon");
    }

    // `Hash` (+ `Eq`) lets `LANG` key a `HashMap` / populate a
    // `HashSet` — the headline use case from issue #508.
    #[test]
    fn lang_is_usable_as_hash_key() {
        use std::collections::{HashMap, HashSet};
        let mut set = HashSet::new();
        for lang in LANG::into_enum_iter() {
            assert!(set.insert(lang), "{} inserted twice", lang.name());
        }
        assert_eq!(set.len(), LANG::into_enum_iter().count());

        let mut map = HashMap::new();
        map.insert(LANG::Rust, "rs");
        map.insert(LANG::Python, "py");
        assert_eq!(map.get(&LANG::Rust), Some(&"rs"));
        assert_eq!(map.get(&LANG::Cpp), None);
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
