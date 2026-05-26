//! Integration tests pinning every `Lang` variant to its expected
//! backing tree-sitter grammar crate and stringified name.
//!
//! Background (issue #350): `mk_get_language!` in `enums/src/macros.rs`
//! is a hand-rolled `match` over `Lang` variants, while the variant
//! list itself lives in `mk_langs!` (`enums/src/languages.rs`). Drift
//! between the two — e.g. the Cpp -> mozcpp swap fixed in #344 — was
//! previously caught only when a runtime caller hit the unhandled arm
//! and panicked. Pinning each variant here turns that class of bug
//! into a compile / `cargo test` failure inside the `enums` crate.
//!
//! Per-variant tests are deliberately verbose (one `#[test]` per
//! variant) so the test name names the language and a reviewer can
//! scan the file to verify each grammar binding. The final
//! `coverage_*` tests use `Lang::into_enum_iter` to guarantee that
//! every variant defined by `mk_langs!` was tested by name above.

use enums::{Lang, get_language, get_language_name};
use tree_sitter::Language;

// `expected` mirrors what `mk_get_language!` does for the variant
// under test. We deliberately repeat the `LANGUAGE.into()` call here
// rather than reuse `get_language` — comparing `get_language(...)` to
// itself would be a tautology and would not catch macro-arm drift.
fn assert_dispatch(lang: Lang, name: &'static str, expected: Language) {
    assert_eq!(get_language(&lang), expected, "{name} grammar mismatch");
    assert_eq!(get_language_name(&lang), name, "{name} name mismatch");
}

#[test]
fn lang_kotlin_resolves_to_tree_sitter_kotlin_ng() {
    assert_dispatch(
        Lang::Kotlin,
        "Kotlin",
        tree_sitter_kotlin_ng::LANGUAGE.into(),
    );
}

#[test]
fn lang_lua_resolves_to_tree_sitter_lua() {
    assert_dispatch(Lang::Lua, "Lua", tree_sitter_lua::LANGUAGE.into());
}

#[test]
fn lang_java_resolves_to_tree_sitter_java() {
    assert_dispatch(Lang::Java, "Java", tree_sitter_java::LANGUAGE.into());
}

#[test]
fn lang_go_resolves_to_tree_sitter_go() {
    assert_dispatch(Lang::Go, "Go", tree_sitter_go::LANGUAGE.into());
}

#[test]
fn lang_rust_resolves_to_tree_sitter_rust() {
    assert_dispatch(Lang::Rust, "Rust", tree_sitter_rust::LANGUAGE.into());
}

// Tcl is backed by the vendored `bca-tree-sitter-tcl` fork; the
// `package = ...` alias in Cargo.toml preserves the `tree_sitter_tcl`
// import path. See enums/Cargo.toml.
#[test]
fn lang_tcl_resolves_to_vendored_tcl() {
    assert_dispatch(Lang::Tcl, "Tcl", tree_sitter_tcl::LANGUAGE.into());
}

// The Cpp -> mozcpp swap is the original drift bug this test file
// guards against (issue #344). Asserting against `tree_sitter_mozcpp`
// (not `tree_sitter_cpp`) is the load-bearing detail here.
#[test]
fn lang_cpp_resolves_to_mozcpp() {
    assert_dispatch(Lang::Cpp, "Cpp", tree_sitter_mozcpp::LANGUAGE.into());
}

#[test]
fn lang_python_resolves_to_tree_sitter_python() {
    assert_dispatch(Lang::Python, "Python", tree_sitter_python::LANGUAGE.into());
}

// Both Tsx and Typescript share the `tree_sitter_typescript` crate
// but pick different `LANGUAGE_*` exports; pinning both ensures the
// macro never collapses them to the same arm.
#[test]
fn lang_tsx_resolves_to_typescript_tsx() {
    assert_dispatch(
        Lang::Tsx,
        "Tsx",
        tree_sitter_typescript::LANGUAGE_TSX.into(),
    );
}

#[test]
fn lang_typescript_resolves_to_typescript_typescript() {
    assert_dispatch(
        Lang::Typescript,
        "Typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    );
}

#[test]
fn lang_bash_resolves_to_tree_sitter_bash() {
    assert_dispatch(Lang::Bash, "Bash", tree_sitter_bash::LANGUAGE.into());
}

// The Csharp variant uses the `tree_sitter_c_sharp` crate (note the
// underscore-separated import name vs the camel-cased enum variant).
#[test]
fn lang_csharp_resolves_to_tree_sitter_c_sharp() {
    assert_dispatch(Lang::Csharp, "Csharp", tree_sitter_c_sharp::LANGUAGE.into());
}

#[test]
fn lang_elixir_resolves_to_tree_sitter_elixir() {
    assert_dispatch(Lang::Elixir, "Elixir", tree_sitter_elixir::LANGUAGE.into());
}

// Vendored fork: bca-tree-sitter-ccomment.
#[test]
fn lang_ccomment_resolves_to_vendored_ccomment() {
    assert_dispatch(
        Lang::Ccomment,
        "Ccomment",
        tree_sitter_ccomment::LANGUAGE.into(),
    );
}

// Vendored fork: bca-tree-sitter-preproc.
#[test]
fn lang_preproc_resolves_to_vendored_preproc() {
    assert_dispatch(
        Lang::Preproc,
        "Preproc",
        tree_sitter_preproc::LANGUAGE.into(),
    );
}

// Vendored Mozilla JS grammar fork: bca-tree-sitter-mozjs.
#[test]
fn lang_mozjs_resolves_to_vendored_mozjs() {
    assert_dispatch(Lang::Mozjs, "Mozjs", tree_sitter_mozjs::LANGUAGE.into());
}

#[test]
fn lang_javascript_resolves_to_tree_sitter_javascript() {
    assert_dispatch(
        Lang::Javascript,
        "Javascript",
        tree_sitter_javascript::LANGUAGE.into(),
    );
}

#[test]
fn lang_perl_resolves_to_tree_sitter_perl() {
    assert_dispatch(Lang::Perl, "Perl", tree_sitter_perl::LANGUAGE.into());
}

// PHP uses `LANGUAGE_PHP` (not the bare `LANGUAGE`), since the crate
// also exposes `LANGUAGE_PHP_ONLY`. Pinning the exact export guards
// against a macro arm flipping between them.
#[test]
fn lang_php_resolves_to_tree_sitter_php_language_php() {
    assert_dispatch(Lang::Php, "Php", tree_sitter_php::LANGUAGE_PHP.into());
}

#[test]
fn lang_ruby_resolves_to_tree_sitter_ruby() {
    assert_dispatch(Lang::Ruby, "Ruby", tree_sitter_ruby::LANGUAGE.into());
}

// Groovy is backed by the `dekobon-tree-sitter-groovy` crate (not
// `tree_sitter_groovy`); the macro arm currently imports it under
// `dekobon_tree_sitter_groovy::LANGUAGE`.
#[test]
fn lang_groovy_resolves_to_dekobon_tree_sitter_groovy() {
    assert_dispatch(
        Lang::Groovy,
        "Groovy",
        dekobon_tree_sitter_groovy::LANGUAGE.into(),
    );
}

// Coverage guard: if a future change adds a new variant to `mk_langs!`
// but forgets to add a per-variant test above, this assertion fails.
// The expected count must be bumped in lockstep with the variant list.
//
// Bumping this number without adding a matching `lang_*_resolves_to_*`
// test above is the failure mode this guard exists to catch.
const EXPECTED_LANG_VARIANT_COUNT: usize = 21;

#[test]
fn coverage_every_lang_variant_is_dispatched() {
    // The count is the load-bearing drift guard. Per-variant `#[test]`
    // functions above already pin each variant's grammar and name —
    // there's no need to re-iterate here. (`mk_get_language!` and
    // `mk_get_language_name!` are exhaustive matches, so a missing
    // arm is a compile error, not a runtime panic.)
    assert_eq!(
        Lang::into_enum_iter().count(),
        EXPECTED_LANG_VARIANT_COUNT,
        "Lang variant count drifted; add a per-variant test in this file \
         and bump EXPECTED_LANG_VARIANT_COUNT to match"
    );
}
