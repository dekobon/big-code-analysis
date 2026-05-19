//! In-source suppression markers for metric threshold checks.
//!
//! This module implements the comment-based suppression scanner
//! described in issue #98. Two dialects coexist:
//!
//! - **Native markers** use the `bca:` namespace and the `suppress`
//!   verb, matching the codebase's internal "suppression" vocabulary
//!   (`SuppressionPolicy`, `FuncSpace::suppressed`, `--no-suppress`):
//!   - `bca: suppress` — suppress all metrics for the enclosing function.
//!   - `bca: suppress(cyclomatic, cognitive)` — suppress only the listed
//!     metrics for the enclosing function.
//!   - `bca: suppress-file` — suppress all metrics for the entire file.
//!   - `bca: suppress-file(halstead)` — suppress listed metrics file-wide.
//! - **Lizard compatibility markers** are recognized verbatim so
//!   existing Lizard-instrumented codebases migrate without rewrites:
//!   - `#lizard forgives` ≡ `bca: suppress`.
//!   - `#lizard forgive global` ≡ `bca: suppress-file`.
//!
//! Markers are extracted from comment nodes during the AST walk in
//! [`crate::spaces::metrics_with_options`] and attached to the matching
//! [`crate::FuncSpace::suppressed`] field. Metric computation is
//! unaffected — suppression is a *threshold-check* concern, not a
//! *measurement* concern, so raw JSON / YAML output still reports every
//! number.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::Serialize;

/// Stable metric identifier set that suppression markers can name.
///
/// Names match the JSON field names emitted on [`crate::CodeMetrics`]
/// (and on the per-metric `bca` threshold registry). Unknown
/// identifiers in a `bca: suppress(...)` list produce a hard error so a
/// typo cannot silently widen suppression scope to other metrics or be
/// dropped on the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// Cognitive complexity.
    Cognitive,
    /// Cyclomatic complexity (both standard and modified variants).
    Cyclomatic,
    /// Halstead suite.
    Halstead,
    /// Lines-of-code suite (sloc, ploc, lloc, cloc, blank).
    Loc,
    /// Maintainability Index suite.
    Mi,
    /// Number of arguments.
    Nargs,
    /// Number of methods / functions.
    Nom,
    /// Number of public attributes.
    Npa,
    /// Number of public methods.
    Npm,
    /// ABC (assignments, branches, conditions) magnitude.
    Abc,
    /// Number of exit points.
    Exit,
    /// Weighted methods per class.
    Wmc,
}

/// Whether downstream consumers (threshold checking, audit logging)
/// should honor parsed suppression markers.
///
/// `Honor` is the default behaviour for `bca check` runs; `Ignore`
/// powers the `--no-suppress` CLI flag so CI auditors can see the raw,
/// un-silenced offender list without editing source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionPolicy {
    /// Skip violations whose metric is covered by an applicable marker.
    Honor,
    /// Emit every violation regardless of markers.
    Ignore,
}

impl SuppressionPolicy {
    /// Construct from a boolean `no_suppress` flag, as parsed from the
    /// CLI. `true` means "ignore markers" (`--no-suppress` set);
    /// `false` means "honor markers" (the default).
    #[must_use]
    pub const fn from_no_suppress(no_suppress: bool) -> Self {
        if no_suppress {
            Self::Ignore
        } else {
            Self::Honor
        }
    }
}

impl MetricKind {
    /// Resolve a sub-metric threshold name (e.g. `cyclomatic.modified`,
    /// `halstead.volume`, `loc.lloc`) to its parent [`MetricKind`].
    ///
    /// The threshold engine uses dotted forms to address individual
    /// sub-metrics, but suppression markers only know about the
    /// top-level metric family — silencing `halstead` silences all of
    /// `halstead.volume`, `halstead.effort`, etc. This translation
    /// happens here so the threshold-check loop can ask one question
    /// ("does this scope cover this metric family?") instead of
    /// special-casing each dotted name.
    #[must_use]
    pub fn for_threshold_name(name: &str) -> Option<Self> {
        // Strip the dotted sub-metric suffix if present. `name` like
        // `halstead.volume` becomes `halstead`; `nom` stays as-is.
        let family = name.split_once('.').map_or(name, |(prefix, _)| prefix);
        // `nexits` is the threshold-engine spelling for what the
        // suppression vocabulary calls `exit` (matching the issue's
        // explicit list). Alias it here rather than splitting one
        // metric into two suppression identifiers.
        let canonical = match family {
            "nexits" => "exit",
            "tokens" => return None,
            other => other,
        };
        Self::from_str(canonical).ok()
    }

    /// Canonical string form. Round-trips through [`FromStr`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cognitive => "cognitive",
            Self::Cyclomatic => "cyclomatic",
            Self::Halstead => "halstead",
            Self::Loc => "loc",
            Self::Mi => "mi",
            Self::Nargs => "nargs",
            Self::Nom => "nom",
            Self::Npa => "npa",
            Self::Npm => "npm",
            Self::Abc => "abc",
            Self::Exit => "exit",
            Self::Wmc => "wmc",
        }
    }

    /// Every [`MetricKind`] variant, in alphabetical order. Used to
    /// render the "known metrics:" hint in error messages; the test
    /// `metric_kind_all_is_alphabetical` locks the order so the hint
    /// stays predictable across releases.
    pub const ALL: &'static [Self] = &[
        Self::Abc,
        Self::Cognitive,
        Self::Cyclomatic,
        Self::Exit,
        Self::Halstead,
        Self::Loc,
        Self::Mi,
        Self::Nargs,
        Self::Nom,
        Self::Npa,
        Self::Npm,
        Self::Wmc,
    ];
}

impl fmt::Display for MetricKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MetricKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|m| m.as_str() == s)
            .ok_or(())
    }
}

/// Which metrics a suppression marker covers.
///
/// `All` means the marker omits an explicit metric list and therefore
/// silences every threshold for the enclosing scope. `Some` carries
/// the explicit list parsed from `bca: suppress(a, b, c)`; an empty set
/// means the marker effectively suppresses nothing (only possible via
/// an empty `()` list, which is treated as a no-op rather than an
/// error).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "metrics")]
pub enum SuppressionScope {
    /// Suppress every metric.
    All,
    /// Suppress only the listed metrics.
    Some(BTreeSet<MetricKind>),
}

impl Default for SuppressionScope {
    /// The default scope suppresses nothing — empty `Some` so newly
    /// constructed `FuncSpace`s carry "no suppressions" without having
    /// to allocate.
    fn default() -> Self {
        Self::Some(BTreeSet::new())
    }
}

impl SuppressionScope {
    /// True when the scope suppresses every metric.
    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// True when the scope suppresses nothing — used by serde to elide
    /// the field from JSON output when no markers fired.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Some(s) if s.is_empty())
    }

    /// True when this scope suppresses `metric`.
    #[must_use]
    pub fn covers(&self, metric: MetricKind) -> bool {
        match self {
            Self::All => true,
            Self::Some(s) => s.contains(&metric),
        }
    }

    /// Merge `other` into `self`. `All` absorbs everything; otherwise
    /// the two sets union. Used when multiple markers stack on the
    /// same function or file.
    pub(crate) fn merge(&mut self, other: &SuppressionScope) {
        match (&mut *self, other) {
            (Self::All, _) => {}
            (slot, Self::All) => *slot = Self::All,
            (Self::Some(a), Self::Some(b)) => a.extend(b.iter().copied()),
        }
    }
}

/// Whether a marker applies to the enclosing function or to the
/// whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuppressionKind {
    /// Suppress thresholds for the function the comment lives in.
    Function,
    /// Suppress thresholds for the whole file.
    File,
}

/// Which dialect surfaced this suppression — useful for the audit log
/// so projects can migrate Lizard-style markers over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SuppressionSource {
    /// Native `bca:` marker.
    Native,
    /// Lizard compatibility marker.
    Lizard,
}

/// A single suppression directive parsed from a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Suppression {
    /// Function- vs file-scoped.
    pub(crate) kind: SuppressionKind,
    /// Which metrics the marker covers.
    pub(crate) scope: SuppressionScope,
    /// Native vs Lizard dialect.
    pub(crate) source: SuppressionSource,
}

/// Error returned when a marker is recognized as a `bca:` directive but
/// the body is malformed (unknown verb, malformed list, unknown metric
/// identifier). Lizard-style markers never error: anything that does
/// not match the exact `#lizard forgives` / `#lizard forgive global`
/// shapes simply parses as "not a marker".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SuppressionError {
    /// `bca:` directive used an unrecognized verb (anything other than
    /// `suppress` / `suppress-file`).
    UnknownVerb(String),
    /// `bca: suppress(...)` listed an identifier that is not a known
    /// metric name.
    UnknownMetric(String),
    /// `bca: suppress(...)` body could not be tokenized (e.g. unbalanced
    /// parentheses, stray characters).
    MalformedBody(String),
}

impl fmt::Display for SuppressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Single-quote delimiters keep the rendered identifier readable
        // without the `{:?}`-style escaping that would otherwise wrap
        // user-supplied verb / metric tokens in literal backslashes.
        match self {
            Self::UnknownVerb(v) => write!(
                f,
                "unknown bca directive verb '{v}'; expected `suppress` or `suppress-file`"
            ),
            Self::UnknownMetric(m) => {
                let known = MetricKind::ALL
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "unknown metric '{m}' in bca suppression marker; known metrics: {known}"
                )
            }
            Self::MalformedBody(body) => {
                write!(f, "malformed bca suppression marker body '{body}'")
            }
        }
    }
}

impl std::error::Error for SuppressionError {}

/// Parse a single comment's text and try to extract a suppression
/// directive. Returns:
///
/// - `Ok(None)` when the comment carries no marker (the common case).
/// - `Ok(Some(s))` when a marker was successfully parsed.
/// - `Err(e)` only for *native* markers whose body is malformed —
///   Lizard-style markers never error.
///
/// The input is the raw comment text **including** the comment-syntax
/// delimiters (e.g. `// bca: suppress`, `# bca: suppress`, `/* bca: suppress */`).
/// The following leading delimiter characters are stripped before
/// matching so per-language wrappers do not have to normalise:
/// `/`, `*`, `!`, `#`, `;`, `-`, and ASCII whitespace. The `!` entry
/// covers Rust inner doc comments (`//!`, `/*!`); the `;` and `-`
/// entries cover Lisp / SQL / Lua line-comment shapes.
pub(crate) fn parse_marker(comment_text: &str) -> Result<Option<Suppression>, SuppressionError> {
    // Fast-bail: this function runs on every comment node. Most
    // comments are license headers, doc comments, or TODO notes that
    // contain neither sigil. `str::contains` is SIMD-accelerated and
    // avoids the trim/strip chain below for the dominant case.
    if !comment_text.contains("bca:") && !comment_text.contains("lizard") {
        return Ok(None);
    }

    // Strip a `/*` opener and a `*/` closer if present so we don't
    // confuse block-comment delimiters with marker prefixes.
    let trimmed = strip_block_delims(comment_text.trim()).trim();

    // Strip language-level comment openers *other than* `#`. We can't
    // strip `#` here because Lizard's marker shape (`#lizard
    // forgives`) needs the `#` to remain. In C++ `// #lizard ...`
    // the `// ` must come off first so Lizard parsing sees `#lizard
    // ...`. In Python `# #lizard ...` (the outer `#` is the language
    // comment opener) tree-sitter delivers the raw `# #lizard ...`
    // text — so the inner body still starts with `#`, which Lizard
    // parsing wants. In both cases the no-`#` trim leaves the
    // `#lizard` token intact.
    // `!` is included so inner doc comments — `//! bca: suppress` and
    // `/*! bca: suppress */` — strip down to the same body as their
    // outer counterparts. Without this, the leading `!` would survive
    // the strip and break the `bca:` prefix match.
    let no_opener = trimmed
        .trim_start_matches(|c: char| {
            c == '/' || c == '*' || c == '!' || c == ';' || c == '-' || c.is_whitespace()
        })
        .trim_end_matches(|c: char| c == '*' || c == '/' || c.is_whitespace())
        .trim();

    // Python-style: tree-sitter delivers `# bca: suppress` with the
    // leading `#` intact. Lizard expects `#lizard ...` — a literal
    // `#` *followed by* `lizard`, no space. If the first `#` is the
    // language's comment opener, strip exactly one `#` and any
    // whitespace before retrying Lizard. The Python `# #lizard ...`
    // shape is then also covered because two `#`s round-trip
    // through one strip + one Lizard `#` prefix.
    //
    // Match `#l` only — Lizard's own scanner is case-sensitive
    // (`parse_lizard` does `strip_prefix("lizard")`), so accepting
    // `#L` here would just defer a failure to `parse_lizard`. Keeping
    // the discriminator lowercase-only also matches the fast-bail
    // above (`contains("lizard")`).
    let lizard_candidate = if no_opener.starts_with("#l") {
        // Already in `#lizard ...` shape after only block-delim
        // stripping — typical for C++ where `// #lizard ...` has
        // had `// ` removed above.
        no_opener
    } else if let Some(rest) = no_opener.strip_prefix('#') {
        // Python/Bash style: `# #lizard ...` or `# bca: ...`. Drop
        // the language comment opener; Lizard parsing only fires
        // when what remains starts with another `#lizard`.
        rest.trim_start()
    } else {
        no_opener
    };

    if let Some(s) = parse_lizard(lizard_candidate) {
        return Ok(Some(s));
    }

    // For native parsing, strip the same `#` opener so `# bca: suppress`
    // matches. The remaining body is then checked for the `bca:`
    // prefix.
    let body = no_opener
        .trim_start_matches(|c: char| c == '#' || c.is_whitespace())
        .trim();

    parse_native(body)
}

fn strip_block_delims(s: &str) -> &str {
    let s = s.strip_prefix("/*").unwrap_or(s);
    s.strip_suffix("*/").unwrap_or(s)
}

fn parse_lizard(trimmed: &str) -> Option<Suppression> {
    // `#lizard forgives` — function-scoped, all metrics.
    // `#lizard forgive global` — file-scoped, all metrics.
    //
    // Lizard's own scanner tolerates a single space after `#` and
    // around the verb, but is otherwise exact. We mirror that:
    // canonicalize whitespace inside the marker, then match literals.
    let s = trimmed.strip_prefix('#')?.trim_start();
    let s = s.strip_prefix("lizard")?;
    let rest = s.trim();

    if rest == "forgives" {
        return Some(Suppression {
            kind: SuppressionKind::Function,
            scope: SuppressionScope::All,
            source: SuppressionSource::Lizard,
        });
    }
    if rest == "forgive global" {
        return Some(Suppression {
            kind: SuppressionKind::File,
            scope: SuppressionScope::All,
            source: SuppressionSource::Lizard,
        });
    }
    None
}

fn parse_native(body: &str) -> Result<Option<Suppression>, SuppressionError> {
    // The native dialect is `bca:` followed by a verb (`suppress` or
    // `suppress-file`), optionally followed by `(metric, metric, ...)`.
    let Some(rest) = body.strip_prefix("bca:") else {
        return Ok(None);
    };
    let rest = rest.trim_start();
    if rest.is_empty() {
        // A bare `bca:` with nothing after it isn't useful; treat as
        // not-a-marker rather than an error so the user can write
        // documentation that mentions the namespace without firing.
        return Ok(None);
    }

    let malformed = || SuppressionError::MalformedBody(body.to_owned());

    // Split into verb + parenthesised body. We accept whitespace
    // between the verb and `(`. The verb is the longest prefix of
    // ASCII letters and `-`.
    let verb_end = rest
        .find(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
        .unwrap_or(rest.len());
    let (verb, after_verb) = rest.split_at(verb_end);
    if verb.is_empty() {
        return Err(malformed());
    }

    let kind = match verb {
        "suppress" => SuppressionKind::Function,
        "suppress-file" => SuppressionKind::File,
        other => return Err(SuppressionError::UnknownVerb(other.to_owned())),
    };

    let after_verb = after_verb.trim_start();
    let scope = if after_verb.is_empty() {
        SuppressionScope::All
    } else if let Some(rest) = after_verb.strip_prefix('(') {
        let close = rest.find(')').ok_or_else(malformed)?;
        let (inside, trailing) = rest.split_at(close);
        // After the `)` only whitespace (and `*/` already trimmed by
        // caller) is allowed. Anything else is a malformed marker:
        // reject so `bca: suppress(loc) garbage` doesn't silently succeed.
        if !trailing[1..].trim().is_empty() {
            return Err(malformed());
        }
        parse_metric_list(inside)?
    } else {
        // Trailing text after the verb that isn't `(...)`: reject.
        return Err(malformed());
    };

    Ok(Some(Suppression {
        kind,
        scope,
        source: SuppressionSource::Native,
    }))
}

fn parse_metric_list(inside: &str) -> Result<SuppressionScope, SuppressionError> {
    let mut set = BTreeSet::new();
    for token in inside.split(',') {
        let name = token.trim();
        if name.is_empty() {
            // Empty `()` or trailing commas: skip. An empty list
            // suppresses nothing — equivalent to the marker being
            // absent. We accept rather than error so authors can
            // comment out parts of a list during editing.
            continue;
        }
        let metric = MetricKind::from_str(name)
            .map_err(|()| SuppressionError::UnknownMetric(name.to_owned()))?;
        set.insert(metric);
    }
    Ok(SuppressionScope::Some(set))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_bare_suppress_covers_all_for_function() {
        let s = parse_marker("// bca: suppress").unwrap().unwrap();
        assert_eq!(s.kind, SuppressionKind::Function);
        assert_eq!(s.source, SuppressionSource::Native);
        assert!(matches!(s.scope, SuppressionScope::All));
    }

    #[test]
    fn native_suppress_with_metric_list() {
        let s = parse_marker("// bca: suppress(cyclomatic, cognitive)")
            .unwrap()
            .unwrap();
        assert_eq!(s.kind, SuppressionKind::Function);
        let SuppressionScope::Some(metrics) = s.scope else {
            panic!("expected Some(...)");
        };
        assert!(metrics.contains(&MetricKind::Cyclomatic));
        assert!(metrics.contains(&MetricKind::Cognitive));
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn native_suppress_file_bare() {
        let s = parse_marker("# bca: suppress-file").unwrap().unwrap();
        assert_eq!(s.kind, SuppressionKind::File);
        assert!(matches!(s.scope, SuppressionScope::All));
    }

    #[test]
    fn native_suppress_file_with_metric_list() {
        let s = parse_marker("/* bca: suppress-file(halstead, loc) */")
            .unwrap()
            .unwrap();
        assert_eq!(s.kind, SuppressionKind::File);
        let SuppressionScope::Some(metrics) = s.scope else {
            panic!("expected Some(...)");
        };
        assert!(metrics.contains(&MetricKind::Halstead));
        assert!(metrics.contains(&MetricKind::Loc));
    }

    #[test]
    fn native_unknown_metric_errors() {
        let err = parse_marker("// bca: suppress(no_such_metric)").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownMetric(_)));
        // The error must mention what was unknown so authors can
        // diagnose typos without reading our source.
        let rendered = err.to_string();
        assert!(rendered.contains("no_such_metric"));
        // And it must list the known metrics so a fix is one
        // copy-paste away.
        assert!(rendered.contains("cyclomatic"));
    }

    #[test]
    fn native_unknown_verb_errors() {
        let err = parse_marker("// bca: disable").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownVerb(_)));
        // The error message must guide the author toward the correct
        // verbs without making them grep our source. Anchor each verb
        // with its surrounding backticks so the bare `suppress` check
        // can't be silently satisfied by the substring inside
        // `suppress-file` — a future message that drops the bare verb
        // and keeps only the compound one would otherwise pass this
        // assertion.
        let rendered = err.to_string();
        assert!(
            rendered.contains("`suppress`"),
            "expected message to name the bare `suppress` verb; got: {rendered}"
        );
        assert!(
            rendered.contains("`suppress-file`"),
            "expected message to name the `suppress-file` verb; got: {rendered}"
        );
    }

    /// Locks the hard rename in issue #263: the previous spelling
    /// `// bca: allow` (and `// bca: allow-file`) must no longer be
    /// recognized. They now fall through to `UnknownVerb`, the same
    /// path as any other typo. A future revert that re-adds the old
    /// verb to the match would silently re-enable old-style markers
    /// in shipped source; this test catches that.
    #[test]
    fn legacy_allow_verb_is_unknown() {
        let err = parse_marker("// bca: allow").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownVerb(v) if v == "allow"));
        let err = parse_marker("// bca: allow-file").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownVerb(v) if v == "allow-file"));
        let err = parse_marker("// bca: allow(cyclomatic)").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownVerb(v) if v == "allow"));
    }

    #[test]
    fn native_malformed_body_errors() {
        // Unbalanced paren.
        assert!(matches!(
            parse_marker("// bca: suppress(cyclomatic").unwrap_err(),
            SuppressionError::MalformedBody(_)
        ));
        // Trailing garbage after the metric list.
        assert!(matches!(
            parse_marker("// bca: suppress(cyclomatic) junk").unwrap_err(),
            SuppressionError::MalformedBody(_)
        ));
        // Verb followed by something other than `(...)`.
        assert!(matches!(
            parse_marker("// bca: suppress garbage").unwrap_err(),
            SuppressionError::MalformedBody(_)
        ));
    }

    #[test]
    fn native_bare_colon_is_not_a_marker() {
        // `bca:` with nothing after it is not a marker; we want to
        // allow documentation comments to mention the namespace.
        assert!(parse_marker("// bca:").unwrap().is_none());
    }

    #[test]
    fn empty_metric_list_is_noop_not_error() {
        let s = parse_marker("// bca: suppress()").unwrap().unwrap();
        assert!(s.scope.is_empty());
        assert!(!s.scope.covers(MetricKind::Cyclomatic));
    }

    #[test]
    fn lizard_function_marker() {
        let s = parse_marker("// #lizard forgives").unwrap().unwrap();
        assert_eq!(s.kind, SuppressionKind::Function);
        assert_eq!(s.source, SuppressionSource::Lizard);
        assert!(matches!(s.scope, SuppressionScope::All));
    }

    #[test]
    fn lizard_file_marker() {
        let s = parse_marker("# #lizard forgive global").unwrap().unwrap();
        assert_eq!(s.kind, SuppressionKind::File);
        assert_eq!(s.source, SuppressionSource::Lizard);
    }

    #[test]
    fn lizard_unknown_phrase_is_not_a_marker() {
        // Per the issue's narrow compat surface: `#lizard skip` is not
        // a recognized Lizard directive, so we treat it as no marker
        // rather than erroring or silently suppressing.
        assert!(parse_marker("// #lizard skip").unwrap().is_none());
    }

    #[test]
    fn plain_comment_is_not_a_marker() {
        assert!(parse_marker("// just a comment").unwrap().is_none());
        assert!(parse_marker("/* TODO: fix later */").unwrap().is_none());
    }

    /// Locks the fast-bail contract in `parse_marker`: comments that
    /// contain neither `bca:` nor `lizard` must short-circuit to
    /// `Ok(None)`. A future change broadening the substring check
    /// (case-insensitive, etc.) would silently shift parsing semantics
    /// for comments that mention `Bca:` or `Lizard` in prose; this
    /// test catches that.
    #[test]
    fn fast_bail_skips_sigil_free_comments() {
        // Long, sigil-free comments that should never trigger.
        assert!(
            parse_marker("// Copyright (c) 2026 Some Corp.")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_marker("/* SPDX-License-Identifier: MIT */")
                .unwrap()
                .is_none()
        );
        // Substring-mention-but-not-a-marker: contains "lizard" in
        // prose but is not a Lizard directive. Slow path must still
        // return Ok(None).
        assert!(
            parse_marker("// authors: jane lizard, john doe")
                .unwrap()
                .is_none()
        );
    }

    /// Locks the case sensitivity of both dialects: `Bca:` and
    /// `#Lizard` must NOT be recognized. Both the fast-bail and the
    /// underlying parsers are lowercase-only by design; this test
    /// pins that contract.
    #[test]
    fn marker_grammar_is_case_sensitive() {
        // Uppercase B in `Bca:` is not a native marker.
        assert!(parse_marker("// Bca: suppress").unwrap().is_none());
        assert!(parse_marker("/* BCA: suppress */").unwrap().is_none());
        // Uppercase L in `#Lizard` is not a Lizard marker. The
        // fast-bail rejects it (no lowercase "lizard" substring) and
        // the slow path would also reject it via `strip_prefix("lizard")`.
        assert!(parse_marker("# #Lizard forgives").unwrap().is_none());
        assert!(parse_marker("// #Lizard forgives").unwrap().is_none());
    }

    #[test]
    fn metric_kind_round_trips() {
        for &m in MetricKind::ALL {
            assert_eq!(MetricKind::from_str(m.as_str()), Ok(m));
        }
    }

    #[test]
    fn metric_kind_all_is_alphabetical() {
        assert!(
            MetricKind::ALL.is_sorted_by_key(|m| m.as_str()),
            "MetricKind::ALL must stay sorted so the error-hint ordering is stable; got {:?}",
            MetricKind::ALL
                .iter()
                .map(|m| m.as_str())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn scope_merge_all_absorbs() {
        let mut a = SuppressionScope::Some(BTreeSet::from([MetricKind::Loc]));
        a.merge(&SuppressionScope::All);
        assert!(a.is_all());

        let mut b = SuppressionScope::All;
        b.merge(&SuppressionScope::Some(BTreeSet::from([MetricKind::Loc])));
        assert!(b.is_all());
    }

    #[test]
    fn scope_merge_some_unions() {
        let mut a = SuppressionScope::Some(BTreeSet::from([MetricKind::Loc]));
        a.merge(&SuppressionScope::Some(BTreeSet::from([
            MetricKind::Cognitive,
        ])));
        assert!(a.covers(MetricKind::Loc));
        assert!(a.covers(MetricKind::Cognitive));
        assert!(!a.covers(MetricKind::Cyclomatic));
    }

    #[test]
    fn scope_covers_respects_all_vs_some() {
        assert!(SuppressionScope::All.covers(MetricKind::Cyclomatic));
        let some = SuppressionScope::Some(BTreeSet::from([MetricKind::Loc]));
        assert!(some.covers(MetricKind::Loc));
        assert!(!some.covers(MetricKind::Cyclomatic));
    }

    #[test]
    fn for_threshold_name_maps_dotted_subnames_to_families() {
        // Cyclomatic.modified and cyclomatic both fall under
        // MetricKind::Cyclomatic — silencing `cyclomatic` covers the
        // modified variant too. Same for halstead.* and loc.*.
        assert_eq!(
            MetricKind::for_threshold_name("cyclomatic"),
            Some(MetricKind::Cyclomatic)
        );
        assert_eq!(
            MetricKind::for_threshold_name("cyclomatic.modified"),
            Some(MetricKind::Cyclomatic)
        );
        assert_eq!(
            MetricKind::for_threshold_name("halstead.volume"),
            Some(MetricKind::Halstead)
        );
        assert_eq!(
            MetricKind::for_threshold_name("loc.lloc"),
            Some(MetricKind::Loc)
        );
    }

    #[test]
    fn for_threshold_name_aliases_nexits_to_exit() {
        // The threshold engine surfaces this metric as `nexits`; the
        // suppression vocabulary uses `exit`. The translation must
        // happen here so `bca: suppress(exit)` silences a `nexits`
        // threshold violation as authors expect.
        assert_eq!(
            MetricKind::for_threshold_name("nexits"),
            Some(MetricKind::Exit)
        );
    }

    #[test]
    fn for_threshold_name_returns_none_for_unknown() {
        // `tokens` is in the threshold registry but explicitly absent
        // from the suppression metric set (the issue's list does not
        // include it). Treat as "no metric family" so a marker can't
        // silence the threshold; this is conservative — the issue
        // says unknown identifiers must error, but here we're going
        // the other direction (threshold-name → MetricKind) so the
        // safe choice is "no mapping, no silencing".
        assert_eq!(MetricKind::for_threshold_name("tokens"), None);
        assert_eq!(MetricKind::for_threshold_name("no_such_metric"), None);
    }

    #[test]
    fn default_scope_is_empty() {
        let d = SuppressionScope::default();
        assert!(d.is_empty());
        assert!(!d.is_all());
    }

    #[test]
    fn inner_doc_comments_recognized() {
        // Rust inner doc comments (`//!`, `/*!`) are the same shape as
        // their outer counterparts (`///`, `/**`) modulo the `!` byte.
        // Without `!` in the leading-strip set the marker prefix `bca:`
        // would not match. Both line- and block-comment variants must
        // round-trip the same way.
        let line = parse_marker("//! bca: suppress").unwrap().unwrap();
        assert_eq!(line.kind, SuppressionKind::Function);
        assert!(matches!(line.scope, SuppressionScope::All));

        let block = parse_marker("/*! bca: suppress */").unwrap().unwrap();
        assert_eq!(block.kind, SuppressionKind::Function);
        assert!(matches!(block.scope, SuppressionScope::All));
    }
}
