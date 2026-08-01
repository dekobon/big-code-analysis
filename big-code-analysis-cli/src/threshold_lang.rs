//! Per-language threshold overrides (`[thresholds.lang.<slug>]`, issue #1141).
//!
//! A single `[thresholds]` table has to fit every language in the tree,
//! and the measured spread is too wide for that: the 97.5th-percentile
//! per-function `cognitive` value runs from 4 in C# to 50 in C. This
//! module owns the two halves of the fix — the slug vocabulary and its
//! parse, and the [`LanguageThresholds`] lookup the check walk consults
//! once per file.
//!
//! Two rules point in opposite directions and are easy to conflate:
//!
//! - An **unknown slug written in the manifest** is a hard error. A
//!   typo'd `[thresholds.lang.rust-lang]` would otherwise leave the user
//!   believing a gate is loosened when it is not.
//! - A **recognised language with no override of its own** falls through
//!   to the global table, so nothing is silently ungated.
//!
//! The `preproc` / `ccomment` pseudo-grammars sit outside both rules:
//! `bca check` never gates them at all, so their slugs are rejected
//! rather than accepted as tables that could never fire. A file whose
//! extension maps to no grammar is likewise skipped by the walk before
//! the gate sees it.

use std::collections::BTreeMap;
use std::sync::Arc;

use big_code_analysis::{LANG, Metric};

use crate::thresholds::{ThresholdSet, threshold_scalar};

/// Reserved key inside `[thresholds]` that introduces the per-language
/// override sub-tables (`[thresholds.lang.<slug>]`). The nesting under a
/// reserved `lang` key — rather than a flat `[thresholds.<slug>]` —
/// keeps every future reserved key (`soft` and its successors) out of
/// the language namespace, where a collision would be silent.
pub(crate) const LANG_SUBTABLE_KEY: &str = "lang";

/// Canonical language slugs accepted as `[thresholds.lang.<slug>]` keys,
/// sorted for the did-you-mean hint and the error listing.
///
/// Derived from [`LANG::name`] — the same values [`LANG`]'s `FromStr`
/// matches and [`crate::walk::valid_languages`] lists for `--language` —
/// so the manifest and the flag cannot grow two spellings of one
/// language. `slug_vocabulary_matches_the_language_flag` pins that.
pub(crate) fn known_language_slugs() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = LANG::into_enum_iter().map(|lang| lang.name()).collect();
    names.sort_unstable();
    names
}

/// Parse the `[thresholds.lang]` sub-table into one `metric = limit` map
/// per language slug, keyed by [`LANG::name`].
///
/// Only the metrics a language actually overrides appear in its map; the
/// rest are inherited from the global table when the set is resolved.
/// An empty override table is dropped rather than recorded, so it
/// behaves exactly like an absent one instead of showing up in
/// `--print-effective-config` as a language whose limits differ.
pub(crate) fn parse_language_tables(
    value: &toml::Value,
) -> Result<BTreeMap<&'static str, BTreeMap<String, f64>>, String> {
    let tables = value.as_table().ok_or_else(|| {
        "[thresholds.lang] must be a table of per-language sub-tables \
         (e.g. `[thresholds.lang.rust]`)"
            .to_owned()
    })?;
    let mut out = BTreeMap::new();
    for (slug, table) in tables {
        let lang = parse_slug(slug)?;
        let limits = parse_one_language_table(slug, table)?;
        if !limits.is_empty() {
            out.insert(lang.name(), limits);
        }
    }
    Ok(out)
}

/// Parse one `[thresholds.lang.<slug>]` table's `metric = limit` pairs.
/// `slug` is used only to attribute errors to the table they came from.
fn parse_one_language_table(
    slug: &str,
    table: &toml::Value,
) -> Result<BTreeMap<String, f64>, String> {
    let table = table.as_table().ok_or_else(|| {
        format!("[thresholds.lang.{slug}] must be a table of `metric = <number>` entries")
    })?;
    let context = format!("[thresholds.lang.{slug}]");
    let mut limits = BTreeMap::new();
    for (name, value) in table {
        // `soft` is the one wrong guess a reader is likely to make here,
        // and the generic "expected a number, got table" would send them
        // looking for a typo rather than at the design.
        if name == crate::threshold_soft::SOFT_SUBTABLE_KEY {
            return Err(format!(
                "[thresholds.lang.{slug}.soft] is not a table: a language's soft tier is \
                 derived from its own hard limits, so `--tier=soft` already scales \
                 [thresholds.lang.{slug}] without one"
            ));
        }
        limits.insert(name.clone(), threshold_scalar(&context, name, value)?);
    }
    Ok(limits)
}

/// Resolve one `[thresholds.lang]` key to its [`LANG`], with the same
/// error shape and did-you-mean hint the unknown-metric path produces.
///
/// The `preproc` and `ccomment` pseudo-grammars parse successfully but
/// are rejected here: `bca check` excludes them from the gate entirely,
/// so a table naming one is a threshold that can never fire — exactly
/// the silent no-op the unknown-slug error exists to prevent.
fn parse_slug(slug: &str) -> Result<LANG, String> {
    let lang: LANG = slug.parse().map_err(|_| {
        let known = known_language_slugs();
        format!(
            "unknown language {slug:?} in [thresholds.lang]{}; known languages: {}",
            crate::threshold_suggestion::format_suggestion(slug, &known),
            known.join(", ")
        )
    })?;
    if matches!(lang, LANG::Preproc | LANG::Ccomment) {
        return Err(format!(
            "[thresholds.lang.{slug}] has no effect: {slug} is an auxiliary grammar that \
             `bca check` never gates, so a limit set here could never fire"
        ));
    }
    Ok(lang)
}

/// The resolved gate: one global [`ThresholdSet`] plus one *fully
/// resolved* set per language carrying an override.
///
/// Per-language sets are complete, not deltas — each is the global table
/// with that language's overrides applied per metric — so selecting one
/// is a lookup, never a merge, and `--print-effective-config` can print
/// the number that will actually apply.
#[derive(Debug)]
pub(crate) struct LanguageThresholds {
    global: Arc<ThresholdSet>,
    /// Keyed by canonical slug ([`LANG::name`]) rather than by [`LANG`],
    /// which is not `Ord`. The string keys also give
    /// `--print-effective-config` a deterministic slug-sorted order for
    /// free.
    per_language: BTreeMap<&'static str, Arc<ThresholdSet>>,
}

impl LanguageThresholds {
    pub(crate) fn new(
        global: ThresholdSet,
        per_language: BTreeMap<&'static str, ThresholdSet>,
    ) -> Self {
        Self {
            global: Arc::new(global),
            per_language: per_language
                .into_iter()
                .map(|(slug, set)| (slug, Arc::new(set)))
                .collect(),
        }
    }

    /// The set that gates `lang`. Falls back to the global set for every
    /// language without an override — including any a future grammar
    /// adds — so there is exactly one selection path and no language can
    /// end up ungated by omission.
    pub(crate) fn for_language(&self, lang: LANG) -> &ThresholdSet {
        self.per_language.get(lang.name()).unwrap_or(&self.global)
    }

    /// The global set, for surfaces that gate nothing language-specific
    /// (`--print-effective-config`'s `[thresholds]` table).
    pub(crate) fn global(&self) -> &ThresholdSet {
        &self.global
    }

    /// The per-language sets, slug-sorted.
    pub(crate) fn languages(&self) -> impl Iterator<Item = (&'static str, &ThresholdSet)> {
        self.per_language
            .iter()
            .map(|(slug, set)| (*slug, set.as_ref()))
    }

    /// True when no tier of this gate configures a single threshold. A
    /// per-language set inherits the global table, so it can only be
    /// empty when the global one is — but check every set anyway, since
    /// a future layering could break that and a silently empty gate
    /// green-lights CI.
    pub(crate) fn is_empty(&self) -> bool {
        self.global.is_empty() && self.per_language.values().all(|set| set.is_empty())
    }

    /// The slugs gated by an override when the global table configures
    /// nothing — i.e. the *only* languages this run gates.
    ///
    /// A manifest with `[thresholds.lang.c]` and no `[thresholds]` is
    /// legal and does exactly what it says, but every other language in
    /// the tree is then walked and gated against an empty set: no
    /// offenders, exit 0, no signal. Before per-language tables an empty
    /// gate always died, so the caller warns rather than let that
    /// difference pass unremarked.
    pub(crate) fn languages_gated_without_a_global_table(&self) -> Option<Vec<&'static str>> {
        if !self.global.is_empty() {
            return None;
        }
        let gated: Vec<&'static str> = self
            .per_language
            .iter()
            .filter(|(_, set)| !set.is_empty())
            .map(|(slug, _)| *slug)
            .collect();
        (!gated.is_empty()).then_some(gated)
    }

    /// The union of every set's metric families, for the `--metrics`-style
    /// narrowing the check walk applies (#1113). Must be the union, not
    /// the global set's families: a metric that only a
    /// `[thresholds.lang.<slug>]` table gates would otherwise be left at
    /// its zero default and silently disarm that language's gate.
    pub(crate) fn selected_metrics(&self) -> Vec<Metric> {
        let mut selected = self.global.selected_metrics();
        for set in self.per_language.values() {
            for metric in set.selected_metrics() {
                if !selected.contains(&metric) {
                    selected.push(metric);
                }
            }
        }
        selected
    }
}

#[cfg(test)]
// Threshold limits are exact `f64` config values, never computed, so
// comparing them is the contract rather than a float-precision hazard.
#[allow(clippy::float_cmp)]
#[path = "threshold_lang_tests.rs"]
mod tests;
