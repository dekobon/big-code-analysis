//! Language detection helpers exposed to Python.
//!
//! These thin wrappers reuse the upstream `LANG` enum and its
//! [`big_code_analysis::guess_language`] helper directly for both
//! extension matching and the shebang / emacs-mode fallback the
//! `bca` CLI walker uses. The forward lookup (variant → name) is
//! owned by [`lang_to_name`] in this module: the upstream
//! `LANG::get_name` returns a display string shared across variants
//! (both `Tsx` and `Typescript` report `"typescript"`), so the
//! Python facade carries its own variant-keyed name table to keep
//! the two disambiguated.

use std::path::Path;

use big_code_analysis::{LANG, guess_language};

use crate::analysis::AnalysisError;

/// Returns the Python-facing language identifier for `lang`.
///
/// The upstream `LANG::get_name` returns a *display* name shared
/// across variants — both `LANG::Tsx` and `LANG::Typescript` report
/// `"typescript"`, both `LANG::Mozjs` and `LANG::Javascript` report
/// `"javascript"` — which makes it ambiguous as a *lookup* key when
/// two variants would round-trip differently through
/// `parse_language_name`. The Python bindings disambiguate by
/// preferring the upstream display name when only one variant in a
/// display group is actually reachable (no helper-variant collision),
/// and falling back to the lowercase Rust variant name otherwise:
///
/// - `Mozjs` is exposed as `"javascript"` — matches the CLI's
///   `"language"` field on every `.js` / `.jsm` / `.mjs` / `.jsx`
///   file. `LANG::Javascript` exists as a placeholder for a future
///   strict-ECMAScript dispatch but has no registered extensions
///   and is filtered out by [`public_languages`], so the shared
///   `"javascript"` name is unambiguous from the Python API.
/// - `Tsx` and `Typescript` get distinct variant names (`"tsx"`,
///   `"typescript"`) because both are reachable (TSX via `.tsx`
///   files, TypeScript via `.ts`) and the CLI display collision
///   would lose information at the API boundary.
/// - `Csharp` is exposed as `"csharp"`.
/// - All other variants use their variant name lowercased
///   (`Rust` → `"rust"`, `Java` → `"java"`, …).
pub(crate) fn lang_to_name(lang: LANG) -> &'static str {
    match lang {
        LANG::Bash => "bash",
        LANG::Ccomment => "ccomment",
        LANG::Cpp => "cpp",
        LANG::Csharp => "csharp",
        LANG::Elixir => "elixir",
        LANG::Go => "go",
        LANG::Groovy => "groovy",
        LANG::Irules => "irules",
        LANG::Java => "java",
        // `Javascript` has no extensions and is filtered out of
        // `public_languages`, so this arm is never reached through
        // the Python API — but the match must stay exhaustive, and
        // grouping the two `"javascript"` variants together documents
        // the intentional CLI-name alias and keeps clippy
        // (`match_same_arms`) quiet.
        LANG::Javascript | LANG::Mozjs => "javascript",
        LANG::Kotlin => "kotlin",
        LANG::Lua => "lua",
        LANG::Perl => "perl",
        LANG::Php => "php",
        LANG::Preproc => "preproc",
        LANG::Python => "python",
        LANG::Ruby => "ruby",
        LANG::Rust => "rust",
        LANG::Tcl => "tcl",
        LANG::Tsx => "tsx",
        LANG::Typescript => "typescript",
    }
}

/// Returns the language name (as accepted by `analyze_source`) that
/// matches `path`, by extension first and falling back to a
/// `#!`-shebang line or an emacs `-*- mode: … -*-` declaration in the
/// file's leading window.
///
/// This is the same detection pipeline used by
/// [`crate::analysis::analyze_path`] and the `bca` CLI's
/// [`big_code_analysis::guess_language`] helper, so a recognised
/// extension and a recognised shebang both round-trip through the
/// public Python API in lockstep with `bca.analyze`.
///
/// The file is read unconditionally — even when the extension would
/// resolve — so the I/O failure modes (missing file, permission
/// denied, …) are uniform regardless of path shape. A two-stage
/// "ext first, read on miss" implementation would mean a recognised
/// extension never touches the filesystem while an unrecognised one
/// always does; that polarity-by-input is more surprising than a
/// single consistent contract.
///
/// I/O failures are surfaced as [`AnalysisError::Io`] so the Python
/// wrapper in [`crate::lib`] dispatches them through the same
/// `OSError` -> `FileNotFoundError` / `PermissionError` / … pipeline
/// that `analyze` uses (`CPython`'s 3-tuple `OSError` constructor).
pub(crate) fn language_for_file(path: &Path) -> Result<Option<&'static str>, AnalysisError> {
    let code = std::fs::read(path).map_err(|source| AnalysisError::io(source, path))?;
    Ok(guess_language(&code, path).0.map(lang_to_name))
}

/// Iterator over the `LANG` variants exposed to Python.
///
/// "Public" means the variant has at least one registered file
/// extension — internal helper variants without user-facing files
/// (`Ccomment`, `Preproc`) are filtered out, since the Python
/// facade has no way to feed them a file and exposing them on the
/// `language` argument would let callers route arbitrary source
/// through the C-preprocessing pipeline.
fn public_languages() -> impl Iterator<Item = LANG> {
    LANG::into_enum_iter().filter(|lang| !lang.get_extensions().is_empty())
}

/// Returns the supported language names, in declaration order.
///
/// "Supported" here means the variant (a) is exposed to Python (i.e.
/// it has at least one registered file extension — internal helper
/// variants `Ccomment` / `Preproc` without user-facing files are
/// filtered out because they cannot be reached through any extension
/// table) AND (b) is enabled in the current build (its per-language
/// Cargo feature is on). The bindings hard-code
/// `default-features = true` on the `big-code-analysis` dep, so in
/// the shipped wheel every grammar is compiled in and condition (b)
/// is always true. The `is_enabled` filter is defensive: a downstream
/// consumer building the bindings with `--no-default-features
/// --features rust` would otherwise see `supported_languages()` list
/// e.g. `"bash"` while `analyze_source(code, "bash")` raises
/// `UnsupportedLanguageError(LanguageDisabled)` at runtime.
pub(crate) fn supported_languages() -> Vec<&'static str> {
    public_languages()
        .filter(LANG::is_enabled)
        .map(lang_to_name)
        .collect()
}

/// Returns the file extensions that resolve to `name`, or `None` when
/// `name` is not a recognised language.
///
/// The list is sourced from the same `get_from_ext` table the
/// upstream [`big_code_analysis::guess_language`] helper consults
/// for the matching variant; every extension here resolves back to
/// `name` via [`language_for_file`] (assuming the target file
/// exists — `language_for_file` reads the file as of #318, so the
/// round-trip is by extension *plus* I/O, not by string shape
/// alone).
pub(crate) fn language_extensions(name: &str) -> Option<Vec<&'static str>> {
    parse_language_name(name).map(|lang| lang.get_extensions().to_vec())
}

/// Resolve a user-supplied language name (as accepted by
/// `analyze_source`) to its `LANG` enum value.
///
/// Matches case-insensitively against [`lang_to_name`]. Helper
/// variants (`Ccomment`, `Preproc`) are *not* exposed through this
/// path — they exist purely to support the C/C++ preprocessing
/// pipeline internally and have no public file extensions, so
/// accepting them as an explicit `language` argument would let
/// callers run them on inputs they were never meant to see.
/// Returns `None` for unknown or internal names; callers map that
/// to `UnsupportedLanguageError` on the Python side.
pub(crate) fn parse_language_name(name: &str) -> Option<LANG> {
    let needle = name.to_lowercase();
    public_languages().find(|lang| lang_to_name(*lang) == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Materialise an empty (or shebang-prefixed) fixture file so the
    /// new `language_for_file` — which reads the source for shebang /
    /// emacs-mode detection — can resolve a path without the
    /// individual tests duplicating tempdir boilerplate.
    fn write_fixture(name: &str, contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, contents).expect("write fixture");
        (dir, path)
    }

    /// Convenience: assert `language_for_file(path)` resolves to `expected`,
    /// unwrapping the `Result` arm with a label so test failures point at
    /// the call site rather than a bare `unwrap`.
    fn assert_language(path: &Path, expected: Option<&str>) {
        let got = language_for_file(path).expect("read fixture");
        assert_eq!(got, expected, "language_for_file({})", path.display());
    }

    #[test]
    fn language_for_file_recognises_rust() {
        let (_dir, path) = write_fixture("foo.rs", b"fn main() {}\n");
        assert_language(&path, Some("rust"));
    }

    #[test]
    fn language_for_file_recognises_js_as_javascript() {
        // CLI parity: `bca metrics --output-format json foo.js`
        // reports `"language": "javascript"` (via
        // `Mozjs.get_name()`). The bindings must round-trip the
        // same string so a user reading the CLI output and feeding
        // it back through `analyze_source` does not hit
        // UnsupportedLanguageError.
        //
        // Cover every extension Mozjs registers in `langs.rs`
        // (`[js, jsm, mjs, jsx]`) — missing one is a silent
        // coverage gap (audit finding A2).
        for ext in ["js", "jsm", "mjs", "jsx"] {
            let (_dir, path) = write_fixture(&format!("foo.{ext}"), b"// js\n");
            assert_language(&path, Some("javascript"));
        }
    }

    #[test]
    fn language_for_file_returns_none_for_unknown_extension() {
        let (_dir, path) = write_fixture("foo.xyz", b"noise\n");
        assert_language(&path, None);
    }

    #[test]
    fn language_for_file_returns_none_for_no_extension_and_no_shebang() {
        // Pin the "no signals at all" path: extension-less, no
        // shebang, no emacs-mode comment → resolves to `None`. Any
        // accidental "default to <some lang>" behaviour upstream
        // would surface here.
        let (_dir, path) = write_fixture("README", b"plain text\n");
        assert_language(&path, None);
    }

    #[test]
    fn language_for_file_resolves_shebang_for_extension_less_script() {
        // #318: CLI parity. An extension-less file whose leading
        // line is `#!/usr/bin/env python` must resolve to "python".
        // Pre-fix, `language_for_file` was extension-only and
        // returned `None`, while `analyze` on the same path
        // succeeded — the asymmetry this issue closed.
        //
        // Test-via-revert: switching the body back to
        // `get_language_for_file(path).map(lang_to_name)` reverts
        // the function to extension-only and makes this assertion
        // fail with `None`.
        let (_dir, path) = write_fixture("install", b"#!/usr/bin/env python\nprint('ok')\n");
        assert_language(&path, Some("python"));
    }

    #[test]
    fn language_for_file_resolves_bash_shebang() {
        // Second-flavour shebang case: `#!/bin/bash` resolves via a
        // different table entry than `#!/usr/bin/env <interp>`.
        // Covering both interpreter forms makes the regression test
        // load-bearing for any future change to the shebang
        // lookup — a path that only exercised `/usr/bin/env` would
        // miss a regression in the bare-interpreter branch.
        let (_dir, path) = write_fixture("run", b"#!/bin/bash\necho hi\n");
        assert_language(&path, Some("bash"));
    }

    #[test]
    fn language_for_file_extension_wins_over_shebang() {
        // `guess_language` orders extension before shebang. A `.rs`
        // file whose body opens with `#!/usr/bin/env python` must
        // still resolve to Rust — the leading `#!` would be a Rust
        // inner-attribute, not an interpreter directive, and silently
        // re-routing such a file to Python would be a data-corruption
        // bug for any caller analysing real Rust source.
        let (_dir, path) = write_fixture("foo.rs", b"#!/usr/bin/env python\nfn main() {}\n");
        assert_language(&path, Some("rust"));
    }

    #[test]
    fn language_for_file_propagates_io_error_for_missing_file() {
        // #318: the new contract drops "never raises" — a missing
        // file surfaces as `AnalysisError::Io` so the Python wrapper
        // can dispatch to the right `OSError` subclass
        // (`FileNotFoundError` here) instead of collapsing to
        // `None`. Hiding a missing file behind `None` would let
        // typos in caller paths silently route to "no language" —
        // exactly the failure mode this issue fixed for `analyze`.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist.rs");
        let err = language_for_file(&missing).expect_err("missing file must error");
        assert!(
            matches!(&err, AnalysisError::Io { source, path }
                if source.kind() == std::io::ErrorKind::NotFound && path == &missing),
            "expected Io(NotFound) for {}, got {err:?}",
            missing.display(),
        );
    }

    #[test]
    fn supported_languages_includes_python_and_rust() {
        let langs = supported_languages();
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"python"));
        assert!(langs.contains(&"java"));
        // `Mozjs` is exposed under the canonical CLI-display name
        // `"javascript"`, NOT the variant name `"mozjs"`. This is
        // the parity contract — a user who reads `"language":
        // "javascript"` from `bca metrics --output-format json` on
        // a `.js` file can pass that same string back through
        // `analyze_source` and get a result.
        assert!(langs.contains(&"javascript"));
        assert!(
            !langs.contains(&"mozjs"),
            "supported_languages should not advertise the internal variant name"
        );
    }

    #[test]
    fn supported_languages_excludes_helper_variants() {
        let langs = supported_languages();
        // `Ccomment` and `Preproc` are internal helpers for the
        // C/C++ pipeline with no registered extensions.
        assert!(!langs.contains(&"ccomment"));
        assert!(!langs.contains(&"preproc"));
    }

    // The disabled-grammar filter on `supported_languages()` cannot
    // be exercised under the default `all-languages` feature set — a
    // test like `assert!(parse_language_name(name).unwrap().is_enabled())`
    // is trivially true under default features and would only fire
    // under a `--no-default-features --features rust` build, for
    // which no CI job currently exists. The `supported_languages
    // <-> parse_language_name` round-trip is already covered by
    // `language_extensions_round_trips_for_every_supported_language`
    // below (it calls `language_extensions(lang)`, which fans out to
    // `parse_language_name(name)`), so no additional sentinel test
    // is needed today.

    #[test]
    fn tsx_and_typescript_are_distinct_python_identifiers() {
        // Upstream `LANG::get_name` returns "typescript" for both
        // `Tsx` and `Typescript`, which would collide as a lookup
        // key. The Python bindings carry their own variant-keyed
        // name table to keep them disambiguated.
        let langs = supported_languages();
        assert!(langs.contains(&"tsx"));
        assert!(langs.contains(&"typescript"));
        assert!(matches!(parse_language_name("tsx"), Some(LANG::Tsx)));
        assert!(matches!(
            parse_language_name("typescript"),
            Some(LANG::Typescript)
        ));
        // `.tsx` resolves to the Tsx variant; `.ts` to Typescript.
        let (_d1, tsx) = write_fixture("foo.tsx", b"// tsx\n");
        let (_d2, ts) = write_fixture("foo.ts", b"// ts\n");
        assert_language(&tsx, Some("tsx"));
        assert_language(&ts, Some("typescript"));
    }

    #[test]
    fn language_extensions_round_trips_for_every_supported_language() {
        // Every language in `supported_languages` must have its
        // extension list reachable via `language_extensions(name)`,
        // and each of those extensions must resolve back to the
        // same language via `language_for_file`. This guards
        // against drift between the two Python entry points.
        for lang in supported_languages() {
            let exts = language_extensions(lang)
                .unwrap_or_else(|| panic!("language_extensions({lang}) should be Some"));
            assert!(!exts.is_empty(), "language {lang} has no extensions");
            for ext in exts {
                let (_dir, path) = write_fixture(&format!("foo.{ext}"), b"");
                assert_language(&path, Some(lang));
            }
        }
    }

    #[test]
    fn parse_language_name_is_case_insensitive() {
        assert!(matches!(parse_language_name("rust"), Some(LANG::Rust)));
        assert!(matches!(parse_language_name("RUST"), Some(LANG::Rust)));
        assert!(matches!(parse_language_name("Rust"), Some(LANG::Rust)));
        assert!(parse_language_name("bogus").is_none());
    }

    #[test]
    fn parse_language_name_resolves_javascript_to_mozjs() {
        // `Mozjs` is the variant that handles `.js`/`.jsx`/`.mjs`/
        // `.jsm` and reports `"javascript"` as its display name in
        // CLI output. The bindings must accept that same string,
        // not the internal variant name.
        assert!(matches!(
            parse_language_name("javascript"),
            Some(LANG::Mozjs)
        ));
        assert!(matches!(
            parse_language_name("JavaScript"),
            Some(LANG::Mozjs)
        ));
        // The variant name is *not* exposed — `LANG::Javascript`
        // has no extensions and is filtered out by
        // `public_languages`, so the string "mozjs" does not
        // resolve to any LANG.
        assert!(parse_language_name("mozjs").is_none());
    }

    #[test]
    fn parse_language_name_rejects_internal_helper_variants() {
        // `Ccomment` and `Preproc` are reachable via `LANG::get_name`
        // / the variant name table but exist only to support the
        // internal C/C++ preprocessing pipeline. The Python facade
        // must refuse to expose them via the explicit-language path —
        // otherwise callers could route arbitrary source through
        // them and get nonsense metrics back.
        assert!(parse_language_name("ccomment").is_none());
        assert!(parse_language_name("preproc").is_none());
    }
}
