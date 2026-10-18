//! "Did you mean?" suggestions for an unknown name.
//!
//! Moved here from the CLI's `threshold_suggestion` module (#1614) so the
//! Python bindings' `metrics=` error, which resolves through
//! [`crate::metric_catalog::resolve_selector`], suggests exactly what
//! `bca metrics --metrics` does. The CLI's other unknown-name errors
//! (`--threshold`, `[thresholds.lang]`, `diff --metric`) reach
//! [`format_suggestion`] through its `#[doc(hidden)]` re-export at the
//! crate root, so the algorithm exists once.

/// Maximum number of "did you mean?" candidates listed in a single
/// unknown-name error. Three is the sweet spot used by `cargo` and
/// `rustc` — informative without dominating the error message.
const MAX_SUGGESTIONS: usize = 3;

/// Hard cap on edit distance for a candidate to be considered "close".
/// Two edits cover the bulk of human typos (transposition, missing or
/// extra letter, single-character substitution) without dragging in
/// unrelated short names.
const MAX_EDIT_DISTANCE: usize = 2;

/// Minimum input length for the prefix-containment strategy to kick in.
/// Below this every name in the registry would match, drowning out
/// real edit-distance candidates.
const MIN_PREFIX_LEN: usize = 3;

/// Levenshtein distance between two byte strings, with an early exit
/// once the running minimum exceeds `cutoff`. Returns `cutoff + 1` in
/// that case — callers only care whether a value is `<= cutoff`.
fn edit_distance_with_cutoff(a: &str, b: &str, cutoff: usize) -> usize {
    let (long, short) = if a.len() >= b.len() {
        (a.as_bytes(), b.as_bytes())
    } else {
        (b.as_bytes(), a.as_bytes())
    };
    if long.len() - short.len() > cutoff {
        return cutoff + 1;
    }
    let mut prev: Vec<usize> = (0..=short.len()).collect();
    let mut curr: Vec<usize> = vec![0; short.len() + 1];
    for (i, &lc) in long.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, &sc) in short.iter().enumerate() {
            let cost = usize::from(lc != sc);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            row_min = row_min.min(curr[j + 1]);
        }
        if row_min > cutoff {
            return cutoff + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[short.len()]
}

/// Returns the closest names in `candidates` to `input` for use in a
/// "did you mean?" suggestion.
///
/// Two complementary strategies surface a candidate:
///
/// 1. Edit distance with a per-pair cutoff scaled to the longer of
///    `input` / candidate name (`max(len) / 3`, capped by
///    `MAX_EDIT_DISTANCE`). Accepts short-typo cases like
///    `cyclomatc` -> `cyclomatic`.
/// 2. Prefix containment for inputs of length >= `MIN_PREFIX_LEN`.
///    Covers truncations like `cyclic` -> `cyclomatic` that
///    Levenshtein alone would reject.
///
/// At most `MAX_SUGGESTIONS` names are returned, all sharing the best
/// score so the message lists genuine ties rather than arbitrarily
/// ranked near-misses. Empty when no candidate matches.
///
/// Score key: `(rank, tiebreaker)` sorts ascending; lower wins.
/// Rank 0 = edit-distance hit (tiebreaker = distance). Rank 1 = strict
/// prefix hit (input is a prefix of the name); rank 2 = shared-prefix
/// hit. For ranks 1/2 the tiebreaker is `usize::MAX - shared_prefix`
/// so a longer shared prefix sorts earlier.
pub(crate) fn closest_names<'a>(input: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let mut scored: Vec<((u8, usize), &'a str)> = candidates
        .iter()
        .filter_map(|&name| {
            let cutoff = MAX_EDIT_DISTANCE.min(input.len().max(name.len()) / 3);
            if cutoff > 0 {
                let d = edit_distance_with_cutoff(input, name, cutoff);
                if d <= cutoff {
                    return Some(((0, d), name));
                }
            }
            if input.len() < MIN_PREFIX_LEN {
                return None;
            }
            let shared = input
                .bytes()
                .zip(name.bytes())
                .take_while(|(x, y)| x == y)
                .count();
            if shared < MIN_PREFIX_LEN {
                return None;
            }
            // Long shared prefix is the strongest signal a user
            // truncated a longer name (`cyclic` -> `cyclomatic`,
            // shared `cycl`). Rank 1 beats rank 2 when `input` is a
            // strict prefix of `name`.
            let rank = u8::from(!name.starts_with(input)) + 1;
            Some(((rank, usize::MAX - shared), name))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let best = scored.first().map(|(s, _)| *s);
    scored
        .into_iter()
        .take_while(|(s, _)| Some(*s) == best)
        .take(MAX_SUGGESTIONS)
        .map(|(_, name)| name)
        .collect()
}

/// Render the trailing "did you mean?" clause for already-chosen
/// `suggestions`. Empty string when there are none, so the caller can
/// unconditionally concatenate.
pub(crate) fn render_suggestion(suggestions: &[&str]) -> String {
    match suggestions {
        [] => String::new(),
        [one] => format!("; did you mean `{one}`?"),
        many => {
            let quoted: Vec<String> = many.iter().map(|n| format!("`{n}`")).collect();
            format!("; did you mean one of {}?", quoted.join(", "))
        }
    }
}

/// Render the trailing "did you mean?" clause for an unknown name
/// `input` checked against `candidates`. Empty string when no close
/// candidate exists, so the caller can unconditionally concatenate.
///
/// `pub` and `#[doc(hidden)]` only so the CLI's unknown-name errors
/// share the algorithm with the library's
/// [`UnknownSelectorError`](crate::metric_catalog::UnknownSelectorError);
/// it is not part of the stability contract.
#[must_use]
pub fn format_suggestion(input: &str, candidates: &[&str]) -> String {
    render_suggestion(&closest_names(input, candidates))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_catalog::METRICS;

    /// The dotted offender ids — the vocabulary `bca check --threshold`
    /// suggests from, which the CLI pins equal to its extractor table.
    fn metric_ids() -> Vec<&'static str> {
        METRICS.iter().map(|m| m.id).collect()
    }

    #[test]
    fn closest_names_suggests_single_typo() {
        // Levenshtein-1 from `cyclomatic`: the only candidate within the
        // cutoff. The suggester must surface exactly one name, not the
        // whole registry.
        assert_eq!(
            closest_names("cyclomatc", &metric_ids()),
            vec!["cyclomatic"]
        );
    }

    #[test]
    fn closest_names_suggests_dotted_typo() {
        // A typo in the post-dot portion of a compound metric name must
        // still find the right candidate — verifies the suggester is
        // string-based, not segmented on `.`.
        assert_eq!(
            closest_names("halstead.efort", &metric_ids()),
            vec!["halstead.effort"]
        );
    }

    #[test]
    fn closest_names_suggests_truncation() {
        // Truncation case from #381: `cyclic` -> `cyclomatic` (4-edit
        // Levenshtein, but a 4-byte shared prefix). The shared-prefix
        // strategy must rescue this so the error is actionable.
        let suggestions = closest_names("cyclic", &metric_ids());
        assert!(
            suggestions.contains(&"cyclomatic"),
            "expected `cyclomatic` in {suggestions:?}"
        );
    }

    #[test]
    fn closest_names_returns_empty_for_unrelated_input() {
        // Pure garbage input must produce no suggestion so the
        // "unknown metric" error remains the primary signal.
        let suggestions = closest_names("xyznonexistent", &metric_ids());
        assert!(suggestions.is_empty(), "{suggestions:?}");
    }

    #[test]
    fn closest_names_returns_empty_for_very_short_input() {
        // A 1-character input falls below the prefix-strategy minimum
        // and has cutoff 0 under the edit-distance strategy, so it must
        // produce no suggestion. Without this, every short candidate
        // would match by trivial substitution.
        assert_eq!(closest_names("z", &metric_ids()), [] as [&str; 0]);
    }

    #[test]
    fn edit_distance_with_cutoff_short_circuits_far_apart() {
        // Inputs whose length difference alone exceeds the cutoff must
        // be rejected without doing the full DP — verifies the early
        // exit path. `cutoff + 1` is the documented sentinel.
        let d = edit_distance_with_cutoff("ab", "abcdefghij", 2);
        assert!(d > 2, "{d}");
    }

    #[test]
    fn edit_distance_with_cutoff_handles_equal_strings() {
        assert_eq!(edit_distance_with_cutoff("cyclomatic", "cyclomatic", 2), 0);
    }

    /// One arm per suggestion count, so the clause the CLI and the
    /// Python bindings append reads the same for none, one and a tie.
    #[test]
    fn render_suggestion_covers_none_one_and_many() {
        assert_eq!(render_suggestion(&[]), "");
        assert_eq!(render_suggestion(&["dloc"]), "; did you mean `dloc`?");
        assert_eq!(
            render_suggestion(&["npa", "npm"]),
            "; did you mean one of `npa`, `npm`?"
        );
    }

    #[test]
    fn format_suggestion_renders_the_closest_names() {
        assert_eq!(
            format_suggestion("cyclomatc", &metric_ids()),
            "; did you mean `cyclomatic`?"
        );
        assert_eq!(format_suggestion("xyznonexistent", &metric_ids()), "");
    }
}
