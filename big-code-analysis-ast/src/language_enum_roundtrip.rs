//! Drift guard for the generated per-language token enums.
//!
//! This module lives at the crate root rather than under `src/languages/`
//! because that directory is wholly owned by the `enums/` codegen — the
//! `enums-codegen-drift` gate flags any hand-written file there as stale.
//!
//! Each `language_<lang>.rs` exposes two generated conversion tables —
//! `From<u16>` (node-kind id → enum variant) and `From<Enum> for
//! &'static str` (variant → kind name). They are pure lookup tables, so
//! nothing exercises most of their arms unless a fixture happens to
//! contain a node of that exact kind; on a typical run they sit at a few
//! percent line coverage and a silent disagreement with the live grammar
//! (e.g. after a grammar bump renumbers node kinds) goes unnoticed.
//!
//! For every supported language we walk all `0..node_kind_count` ids the
//! grammar defines, round-trip each through both tables, and assert the
//! enum's name string agrees with the grammar for every *visible* kind.
//! This both pins the mapping against drift (the same fear behind the
//! `grammar_version` guard in `src/langs.rs`) and drives the generated
//! tables to near-full coverage.

use crate::langs::LANG;
use crate::languages::*;

/// Round-trips every node-kind id the grammar behind `lang` defines
/// through the generated `From<u16>` and `From<Enum> for &'static str`
/// tables, asserting the enum name matches the grammar for each visible
/// kind. Languages whose grammar feature is disabled in the current
/// build are skipped (the enum surface is always compiled, but
/// `tree_sitter_language` hands back `Err(LanguageDisabled)`).
fn check<E>(lang: LANG)
where
    E: From<u16> + Into<&'static str>,
{
    let Ok(grammar) = lang.tree_sitter_language() else {
        return;
    };
    let count = grammar.node_kind_count();
    for id in 0..count {
        // Node-kind ids are u16 in tree-sitter; `node_kind_count` can
        // never exceed that range, so the cast is lossless.
        let id = u16::try_from(id).expect("node-kind id exceeds u16");
        let variant: E = id.into();
        let name: &'static str = variant.into();
        // Hidden / supertype kinds carry grammar-internal names that the
        // enum deliberately does not mirror; only assert on the visible
        // surface, but still convert above so every arm is exercised.
        if grammar.node_kind_is_visible(id) {
            assert_eq!(
                Some(name),
                grammar.node_kind_for_id(id),
                "{lang:?} kind id {id}: enum name {name:?} disagrees with grammar"
            );
        }
    }
}

macro_rules! roundtrip_tests {
    ($($lang:ident),* $(,)?) => {
        $(
            // The token enum and the `LANG` variant share a name; the
            // type position resolves to the re-exported token enum, the
            // value position to `LANG::$lang`.
            #[test]
            #[allow(non_snake_case)]
            fn $lang() {
                check::<$lang>(LANG::$lang);
            }
        )*
    };
}

// One arm per `mk_langs!` entry in `src/langs.rs`. Keep in sync: a new
// language must gain a round-trip test here too.
roundtrip_tests!(
    Javascript, Mozjs, Java, Go, Kotlin, Lua, Rust, Tcl, Irules, C, Cpp, Mozcpp, Objc, Csharp,
    Elixir, Python, Tsx, Typescript, Bash, Ccomment, Preproc, Perl, Php, Ruby, Groovy,
);
