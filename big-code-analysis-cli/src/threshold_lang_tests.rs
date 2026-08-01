//! Unit tests for [`crate::threshold_lang`].

use super::*;
use crate::Action;
use crate::thresholds::ThresholdSet;
use crate::walk::resolve_language;

/// The two auxiliary grammars `bca check` never gates, and so never
/// accepts as `[thresholds.lang]` keys either.
const UNGATED: [LANG; 2] = [LANG::Preproc, LANG::Ccomment];

/// The manifest slug vocabulary and `--language` must stay one list.
///
/// The failure this pins is a second, drifting spelling table: someone
/// adds a language, wires it into `--language`, and hand-writes a slug
/// map for `[thresholds.lang]` that spells it differently (or omits it),
/// so a manifest override silently never matches any file.
#[test]
fn slug_vocabulary_matches_the_language_flag() {
    let slugs = known_language_slugs();
    for lang in LANG::into_enum_iter() {
        let slug = lang.name();
        assert!(
            slugs.contains(&slug),
            "{slug} is a LANG but not an accepted [thresholds.lang] slug"
        );
        // The same string, fed to `--language`, must resolve to the same
        // variant the slug parser produced.
        assert_eq!(
            resolve_language(Some(slug), &Action::Check),
            Some(lang),
            "--language {slug} and [thresholds.lang.{slug}] disagree"
        );
        if !UNGATED.contains(&lang) {
            assert_eq!(parse_slug(slug), Ok(lang));
        }
    }
    assert_eq!(
        slugs.len(),
        LANG::into_enum_iter().count(),
        "slug list and LANG must be the same size"
    );
}

/// `preproc` and `ccomment` parse as languages but are rejected as
/// override keys.
///
/// `dispatch_check_file` skips both grammars before the threshold set is
/// ever consulted, so a `[thresholds.lang.preproc]` table is a limit
/// that can never fire — the same silent no-op the unknown-slug error
/// exists to prevent, which is why it is an error rather than a table
/// `--print-effective-config` would advertise as live.
#[test]
fn ungated_pseudo_language_slugs_are_rejected() {
    for lang in UNGATED {
        let slug = lang.name();
        assert!(
            slug.parse::<LANG>().is_ok(),
            "{slug} must still be a real LANG, or this test proves nothing"
        );
        let err = parse_slug(slug).expect_err("auxiliary grammars are not gated");
        assert!(
            err.contains(&format!("[thresholds.lang.{slug}] has no effect")),
            "error names the offending table: {err}"
        );
    }
}

/// The awkward spellings the issue called out by name, written as
/// literals so a rename in `mk_langs!` fails here rather than silently
/// invalidating every `bca.toml` in the wild.
#[test]
fn awkward_slugs_keep_their_documented_spelling() {
    for (slug, expected) in [
        ("cpp", LANG::Cpp),
        ("csharp", LANG::Csharp),
        ("objc", LANG::Objc),
        ("tsx", LANG::Tsx),
        ("mozcpp", LANG::Mozcpp),
        ("mozjs", LANG::Mozjs),
    ] {
        assert_eq!(parse_slug(slug), Ok(expected), "slug {slug}");
    }
}

/// An unknown slug is rejected with the same error shape (and
/// did-you-mean hint) the unknown-metric path produces — never a silent
/// no-op that leaves the user believing a gate was loosened.
#[test]
fn unknown_slug_error_includes_suggestion() {
    let err = parse_slug("rustlang").expect_err("`rustlang` is not a language");
    assert!(
        err.contains("unknown language \"rustlang\" in [thresholds.lang]"),
        "error names the offending key and table: {err}"
    );
    assert!(
        err.contains("did you mean `rust`?"),
        "error suggests the near miss: {err}"
    );
    assert!(
        err.contains("known languages: "),
        "error lists the accepted set: {err}"
    );
}

/// Parse a `[thresholds.lang]` body — the fixture is written as if the
/// `[thresholds.lang]` header were already consumed, so `[c]` here is
/// `[thresholds.lang.c]` in a real manifest.
fn parse(toml_src: &str) -> Result<BTreeMap<&'static str, BTreeMap<String, f64>>, String> {
    let table: toml::Table = toml::from_str(toml_src).expect("fixture parses as TOML");
    parse_language_tables(&toml::Value::Table(table))
}

#[test]
fn parses_one_table_per_language() {
    let parsed = parse("[c]\ncognitive = 25\n[elixir]\nnom = 100\n").expect("valid tables");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed["c"]["cognitive"], 25.0);
    assert_eq!(parsed["elixir"]["nom"], 100.0);
    // Only the overridden metric is recorded; inheritance happens later,
    // at resolution, against the global table.
    assert_eq!(parsed["c"].len(), 1);
}

#[test]
fn empty_language_table_is_dropped() {
    // `[thresholds.lang.c]` with no keys resolves to the global set, so
    // recording it would make `--print-effective-config` claim a
    // difference that does not exist.
    assert!(parse("[c]\n").expect("valid table").is_empty());
}

#[test]
fn non_table_shapes_are_rejected() {
    let err = parse_language_tables(&toml::Value::Integer(3)).expect_err("not a table");
    assert!(err.contains("[thresholds.lang] must be a table"), "{err}");

    let table: toml::Table = toml::from_str("c = 25").expect("fixture parses");
    let err =
        parse_language_tables(&toml::Value::Table(table)).expect_err("language value not a table");
    assert!(err.contains("[thresholds.lang.c] must be a table"), "{err}");
}

/// `[thresholds.lang.<slug>.soft]` is the wrong guess a reader is most
/// likely to make, so it gets the design decision rather than the
/// generic "expected a number, got table".
#[test]
fn a_nested_soft_table_explains_why_it_does_not_exist() {
    let err = parse("[c.soft]\ncognitive = 5\n").expect_err("no per-language soft table");
    assert!(
        err.contains("[thresholds.lang.c.soft] is not a table")
            && err.contains("derived from its own hard limits"),
        "error points at the derivation, not a typo: {err}"
    );
}

/// A non-numeric limit is attributed to the language table it was
/// written in, not to the global `[thresholds]`.
#[test]
fn non_numeric_limit_names_the_language_table() {
    let err = parse("[c]\ncognitive = \"lots\"\n").expect_err("string is not a limit");
    assert_eq!(
        err,
        "[thresholds.lang.c] \"cognitive\": expected a number, got string"
    );
}

/// `for_language` has one fallback path, not a table of special cases:
/// any language without an override of its own gates against the global
/// set.
#[test]
fn unoverridden_languages_fall_back_to_the_global_set() {
    let global = ThresholdSet::build(&BTreeMap::from([("cognitive".to_owned(), 15.0)]))
        .expect("global set builds");
    let c = ThresholdSet::build(&BTreeMap::from([("cognitive".to_owned(), 25.0)]))
        .expect("C set builds");
    let resolved = LanguageThresholds::new(global, BTreeMap::from([(LANG::C.name(), c)]));

    let limit_for = |lang| {
        resolved
            .for_language(lang)
            .iter()
            .find(|(name, _)| *name == "cognitive")
            .expect("cognitive is configured")
            .1
    };
    assert_eq!(limit_for(LANG::C), 25.0, "the overridden language");
    assert_eq!(limit_for(LANG::Rust), 15.0, "an un-overridden language");
    assert_eq!(limit_for(LANG::Elixir), 15.0, "another un-overridden one");
}

/// The walk narrows metric computation to the families the gate reads
/// (#1113). A metric that only a per-language table gates must survive
/// that narrowing, or its gate silently reads a zero default.
#[test]
fn selected_metrics_unions_every_language() {
    let global = ThresholdSet::build(&BTreeMap::from([("cognitive".to_owned(), 15.0)]))
        .expect("global set builds");
    let elixir = ThresholdSet::build(&BTreeMap::from([
        ("cognitive".to_owned(), 15.0),
        ("nom".to_owned(), 100.0),
    ]))
    .expect("Elixir set builds");
    let resolved = LanguageThresholds::new(global, BTreeMap::from([(LANG::Elixir.name(), elixir)]));

    let selected = resolved.selected_metrics();
    assert!(
        selected.contains(&Metric::Nom),
        "a language-only metric must be computed: {selected:?}"
    );
    assert!(selected.contains(&Metric::Cognitive), "{selected:?}");
    // Deduplicated: `cognitive` appears in both sets.
    assert_eq!(selected.len(), 2, "{selected:?}");
}
