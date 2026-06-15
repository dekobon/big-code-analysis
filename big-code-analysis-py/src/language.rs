// bca: suppress-file(halstead, nargs, nom)
// Language-name / enum mapping helpers; file-level halstead, summed
// nargs, and method count (nom) are many-fn aggregation artifacts — the
// module is a handful of tiny lookup helpers plus their regression
// tests, none individually complex.

//! Language detection helpers exposed to Python.
//!
//! These thin wrappers reuse the upstream `LANG` enum and its
//! [`big_code_analysis::guess_language`] helper directly for both
//! extension matching and the shebang / emacs-mode fallback the
//! `bca` CLI walker uses. The forward lookup (variant → name) is just
//! [`LANG::name`]: since #540 every variant has a distinct canonical
//! lowercase slug (`Cpp` → `"cpp"`, `Csharp` → `"csharp"`, `Tsx` →
//! `"tsx"`, `Typescript` → `"typescript"`), so the Python facade no
//! longer needs its own variant-keyed name table.

use std::path::Path;

use big_code_analysis::{LANG, get_from_ext, guess_language};

use crate::analysis::AnalysisError;

/// Resolve a bare file extension to its language name, filesystem-free.
///
/// Accepts the extension with or without a leading dot (`"py"` and
/// `".py"` both resolve), normalising it internally before consulting the
/// upstream [`big_code_analysis::get_from_ext`] table — the same table
/// [`language_for_file`]'s extension stage uses. Returns `None` for an
/// unknown extension (no raise, no I/O): a pure table lookup in the
/// extension → language direction, the inverse of
/// [`crate::language::language_extensions`] (issue #682).
pub(crate) fn language_for_extension(ext: &str) -> Option<&'static str> {
    // `get_from_ext` keys on the bare suffix (no dot); strip a single
    // leading dot so `".py"` and `"py"` converge, and lowercase so
    // `"PY"` resolves like the case-insensitive walker would.
    let normalized = ext.strip_prefix('.').unwrap_or(ext).to_lowercase();
    get_from_ext(&normalized).map(|lang| lang.name())
}

/// Resolve `path`'s language by its extension alone — filesystem-free.
///
/// Reads nothing and never raises: a path that does not exist yet (an
/// archive listing, a git-tree entry, a candidate filename) still resolves
/// when its extension is known. Returns `None` for an extension-less path
/// or an unknown extension. This is the `read=False` half of the public
/// `language_for_file` (issue #682); the `read=True` default keeps the
/// content-sniffing [`language_for_file`] behaviour.
pub(crate) fn language_for_path_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    language_for_extension(ext)
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
    Ok(guess_language(&code, path).0.map(|lang| lang.name()))
}

/// Whether a `LANG` variant is a user-facing, Python-selectable
/// language (as opposed to an internal C-family helper).
///
/// `Ccomment` and `Preproc` are the only non-public variants: the
/// Python facade has no way to feed them a file, and exposing them on
/// the `language` argument would let callers route arbitrary source
/// through the C-preprocessing pipeline. The predicate is *not* "has a
/// registered extension" — since #720 the opt-in `Mozcpp` dialect owns
/// zero extensions (it is selected explicitly by name) yet is fully
/// public, so it must remain listed. Single-sourced here and reused by
/// the `_enums.py` codegen (`codegen::language_slugs`, via
/// [`public_languages`]) so the public-language set and the generated
/// `Lang` enum cannot drift. This is the *feature-independent*
/// predicate — it does not consider `is_enabled`, which
/// [`supported_languages`] layers on separately.
pub(crate) fn is_public_language(lang: LANG) -> bool {
    !matches!(lang, LANG::Ccomment | LANG::Preproc)
}

/// Iterator over the `LANG` variants exposed to Python — every variant
/// for which [`is_public_language`] holds.
///
/// This is the **feature-independent** public-language set: it does
/// *not* apply the `is_enabled` Cargo-feature filter that
/// [`supported_languages`] layers on top. The `_enums.py` codegen
/// (`codegen::language_slugs`) mirrors this set, *not*
/// `supported_languages`, because the generated artifact is checked in
/// and must list every public language regardless of which language
/// features a given build enables.
pub(crate) fn public_languages() -> impl Iterator<Item = LANG> {
    LANG::into_enum_iter().filter(|&lang| is_public_language(lang))
}

/// Returns the supported language names, in declaration order.
///
/// "Supported" here means the variant (a) is exposed to Python (i.e.
/// it is not an internal C-family helper — `Ccomment` / `Preproc` are
/// filtered out because they cannot be reached through any file, while
/// the extension-less opt-in `Mozcpp` dialect *is* listed since it is
/// selectable by name) AND (b) is enabled in the current build (its per-language
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
        .map(|lang| lang.name())
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
    parse_language_name(name).map(|lang| lang.extensions().to_vec())
}

/// Resolve a user-supplied language name (as accepted by
/// `analyze_source`) to its `LANG` enum value.
///
/// Matches case-insensitively against [`LANG::name`]. Helper
/// variants (`Ccomment`, `Preproc`) are *not* exposed through this
/// path — they exist purely to support the C/C++ preprocessing
/// pipeline internally and have no public file extensions, so
/// accepting them as an explicit `language` argument would let
/// callers run them on inputs they were never meant to see.
/// Returns `None` for unknown or internal names; callers map that
/// to `UnsupportedLanguageError` on the Python side.
pub(crate) fn parse_language_name(name: &str) -> Option<LANG> {
    let needle = name.to_lowercase();
    public_languages().find(|lang| lang.name() == needle)
}

/// Builds the user-facing message for an unknown language name.
///
/// Mirrors the `metrics=` validation style (`unknown metric: …;
/// valid: …`) so a bad language argument names both the offending
/// input and the full set of accepted values, drawn from the same
/// [`supported_languages`] table the public listing uses.
pub(crate) fn unknown_language_message(input: &str) -> String {
    format!(
        "unknown language '{input}'; supported: {supported}",
        supported = supported_languages().join(", "),
    )
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
    fn language_for_file_recognises_js_extensions() {
        // CLI parity since #507: the standard JS extensions dispatch to
        // the upstream `Javascript` grammar (`"javascript"`), while the
        // Mozilla fork `Mozjs` owns only `.jsm` (`"mozjs"`). A user
        // reading `"language"` from `bca metrics --output-format json`
        // must round-trip the same string back through `analyze_source`,
        // so cover every registered extension for both variants — missing
        // one is a silent coverage gap (audit finding A2).
        for ext in ["js", "mjs", "cjs", "jsx"] {
            let (_dir, path) = write_fixture(&format!("foo.{ext}"), b"// js\n");
            assert_language(&path, Some("javascript"));
        }
        let (_dir, path) = write_fixture("foo.jsm", b"// mozilla js module\n");
        assert_language(&path, Some("mozjs"));
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
        // `get_language_for_file(path).map(LANG::name)` reverts
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
        // Since #507 the JavaScript pair is split and both are public:
        // `Javascript` (the `.js`/`.mjs`/`.cjs`/`.jsx` default) under
        // `"javascript"`, and the Mozilla fork `Mozjs` (owning `.jsm`)
        // under `"mozjs"`. Both round-trip — a user who reads
        // `"language"` from `bca metrics --output-format json` can pass
        // that string back through `analyze_source` and get a result.
        assert!(langs.contains(&"javascript"));
        assert!(langs.contains(&"mozjs"));
    }

    #[test]
    fn supported_languages_excludes_helper_variants() {
        let langs = supported_languages();
        // `Ccomment` and `Preproc` are internal helpers for the
        // C/C++ pipeline with no registered extensions.
        assert!(!langs.contains(&"ccomment"));
        assert!(!langs.contains(&"preproc"));
        // But the opt-in Mozilla C++ dialect `Mozcpp` (#720) also owns
        // zero extensions and yet IS public — it is selected by name.
        // This pins the predicate to "not an internal helper": under
        // the pre-#720 `!extensions().is_empty()` filter, `mozcpp`
        // would be wrongly excluded here and this assertion would fail.
        assert!(langs.contains(&"mozcpp"));
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
        // Since #540 `LANG::name` returns distinct canonical slugs
        // ("tsx" for `Tsx`, "typescript" for `Typescript`), so the two
        // no longer collide as lookup keys and the Python facade needs
        // no override to keep them disambiguated.
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
            if exts.is_empty() {
                // Since #720 the opt-in Mozilla C++ dialect is the sole
                // name-only language: selectable by name, owns no file
                // extension, so it has nothing to round-trip through
                // `language_for_file`. Any *other* extension-less
                // supported language would be a bug.
                assert_eq!(lang, "mozcpp", "unexpected extension-less language {lang}");
                continue;
            }
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
    fn parse_language_name_resolves_js_pair_to_distinct_variants() {
        // Since #507 `"javascript"` resolves to the upstream `Javascript`
        // grammar (the `.js`/`.mjs`/`.cjs`/`.jsx` default) and `"mozjs"`
        // to the opt-in Mozilla fork (`.jsm`). Both are now public, so
        // both string forms resolve — case-insensitively.
        assert!(matches!(
            parse_language_name("javascript"),
            Some(LANG::Javascript)
        ));
        assert!(matches!(
            parse_language_name("JavaScript"),
            Some(LANG::Javascript)
        ));
        assert!(matches!(parse_language_name("mozjs"), Some(LANG::Mozjs)));
        assert!(matches!(parse_language_name("MozJS"), Some(LANG::Mozjs)));
    }

    #[test]
    fn parse_language_name_rejects_internal_helper_variants() {
        // `Ccomment` and `Preproc` are reachable via `LANG::name`
        // / the variant name table but exist only to support the
        // internal C/C++ preprocessing pipeline. The Python facade
        // must refuse to expose them via the explicit-language path —
        // otherwise callers could route arbitrary source through
        // them and get nonsense metrics back.
        assert!(parse_language_name("ccomment").is_none());
        assert!(parse_language_name("preproc").is_none());
    }

    #[test]
    fn cpp_csharp_tsx_expose_canonical_slugs() {
        // Since #540 the Python facade reports the upstream canonical
        // slug verbatim — no override. `Cpp`/`Csharp` previously needed
        // overriding because `name()` returned the unusable display
        // forms "c/c++" / "c#"; `Tsx` because it shared "typescript"
        // with `Typescript`. All three are now distinct lookup tokens.
        assert_eq!(LANG::Cpp.name(), "cpp");
        assert_eq!(LANG::Csharp.name(), "csharp");
        assert_eq!(LANG::Tsx.name(), "tsx");
        assert!(matches!(parse_language_name("cpp"), Some(LANG::Cpp)));
        assert!(matches!(parse_language_name("csharp"), Some(LANG::Csharp)));
        assert!(matches!(parse_language_name("tsx"), Some(LANG::Tsx)));
    }
}
