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
//! [`crate::analyze`] / [`crate::Ast::metrics`] and attached to the
//! matching [`crate::FuncSpace::suppressed`] field. Metric computation is
//! unaffected — suppression is a *threshold-check* concern, not a
//! *measurement* concern, so raw JSON / YAML output still reports every
//! number.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::checker::Checker;
use crate::getter::Getter;
use crate::metric_set::Metric;
use crate::node::Node;
use crate::traits::ParserTrait;

/// Resolve a sub-metric threshold name (e.g. `cyclomatic.modified`,
/// `halstead.volume`, `loc.lloc`) to its parent [`Metric`].
///
/// The threshold engine uses dotted forms to address individual
/// sub-metrics, but suppression markers only know about the top-level
/// metric family — silencing `halstead` silences all of
/// `halstead.volume`, `halstead.effort`, etc. This translation happens
/// here so the threshold-check loop can ask one question ("does this
/// scope cover this metric family?") instead of special-casing each
/// dotted name.
///
/// Returns `None` for `tokens`: it has no configurable threshold and is
/// deliberately absent from the suppressible vocabulary
/// ([`Metric::suppressible`]), so a marker can never silence it.
#[must_use]
pub fn threshold_metric_for_name(name: &str) -> Option<Metric> {
    // Strip the dotted sub-metric suffix if present. `name` like
    // `halstead.volume` becomes `halstead`; `nom` stays as-is.
    let family = name.split_once('.').map_or(name, |(prefix, _)| prefix);
    // `tokens` is in the threshold registry but is not suppressible, so
    // it maps to no metric family. Every other name parses via the
    // canonical `Metric::from_str` — `nexits` is the spelling on both
    // sides now, so no alias bridge is needed (the pre-unification
    // `nexits -> exit` mapping retired with `MetricKind` in #555).
    if family == "tokens" {
        return None;
    }
    family.parse().ok()
}

/// Whether downstream consumers (threshold checking, audit logging)
/// should honor parsed suppression markers.
///
/// `Honor` is the default behaviour for `bca check` runs; `Ignore`
/// powers the `--no-suppress` CLI flag so CI auditors can see the raw,
/// un-silenced offender list without editing source files.
// Deliberately exhaustive: a total binary toggle (honor markers vs
// ignore them). There is no third state to add, so `#[non_exhaustive]`
// would only force callers into a pointless wildcard arm.
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

/// Which metrics a suppression marker covers.
///
/// `All` means the marker omits an explicit metric list and therefore
/// silences every threshold for the enclosing scope. `Some` carries
/// the explicit list parsed from `bca: suppress(a, b, c)`; an empty set
/// means the marker effectively suppresses nothing (only possible via
/// an empty `()` list, which is treated as a no-op rather than an
/// error).
// Deliberately exhaustive: a total model of "everything (`All`) vs an
// explicit set (`Some`)". Any new coverage shape is expressible as a
// `Some(set)` rather than a new variant, so the two cases are closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "metrics")]
pub enum SuppressionScope {
    /// Suppress every metric.
    All,
    /// Suppress only the listed metrics.
    Some(BTreeSet<Metric>),
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
    pub fn covers(&self, metric: Metric) -> bool {
        match self {
            Self::All => true,
            Self::Some(s) => s.contains(&metric),
        }
    }

    /// Merge `other` into `self`. `All` absorbs everything; otherwise
    /// the two sets union. Used when multiple markers stack on the
    /// same function or file, and by report consumers to fold a file's
    /// `suppress-file` scope into each function's own scope (issue #501).
    pub fn merge(&mut self, other: &SuppressionScope) {
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
    /// `bca: suppress(...)` named a real metric that has no configurable
    /// threshold and therefore cannot be suppressed (currently only
    /// `tokens`). Distinct from [`Self::UnknownMetric`] so the author
    /// learns the name parsed but is simply not silenceable.
    NonSuppressibleMetric(String),
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
                // The hint lists the suppressible metrics, derived from
                // `Metric::suppressible()` (the single source of truth for
                // the suppressible vocabulary — it already excludes the
                // non-suppressible `tokens`) rather than re-deriving from
                // `Metric::NAMES` with a hardcoded filter. `suppressible()`
                // iterates declaration order; we sort so the hint stays
                // alphabetised and thus stable across releases.
                let mut names: Vec<String> = Metric::suppressible()
                    .map(|metric| metric.to_string())
                    .collect();
                names.sort_unstable();
                let known = names.join(", ");
                write!(
                    f,
                    "unknown metric '{m}' in bca suppression marker; known metrics: {known}"
                )
            }
            Self::NonSuppressibleMetric(m) => {
                write!(f, "metric '{m}' has no threshold and cannot be suppressed")
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
    // around the verb, but is otherwise exact. We mirror that by
    // trimming the ends and matching the verb phrase verbatim.
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
        // Parse through the canonical `Metric` vocabulary (the same one
        // selection uses) so suppression and selection never drift. A
        // typo surfaces the offending token via `ParseMetricError`
        // (#554). `tokens` parses fine but has no threshold, so reject
        // it with a distinct, actionable error rather than silently
        // accepting a no-op suppression.
        let metric: Metric = name
            .parse()
            .map_err(|_| SuppressionError::UnknownMetric(name.to_owned()))?;
        if metric == Metric::Tokens {
            return Err(SuppressionError::NonSuppressibleMetric(name.to_owned()));
        }
        set.insert(metric);
    }
    Ok(SuppressionScope::Some(set))
}

/// Whether an audited suppression marker applies to its enclosing
/// function or to the whole file.
///
/// The public mirror of the crate-internal `SuppressionKind`; exposed
/// on [`SuppressionMarker`] so the `bca exemptions` audit (issue #386)
/// can report marker scope without leaking the internal type.
// Deliberately exhaustive: function- and file-scope are the only two
// suppression granularities the marker grammar models. A new granularity
// would be a grammar-level change, planned deliberately, not a silent
// additive variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionTarget {
    /// Marker silences thresholds for its enclosing function only.
    Function,
    /// Marker silences thresholds for the whole file.
    File,
}

impl From<SuppressionKind> for SuppressionTarget {
    fn from(kind: SuppressionKind) -> Self {
        match kind {
            SuppressionKind::Function => Self::Function,
            SuppressionKind::File => Self::File,
        }
    }
}

/// Which marker dialect produced a suppression.
///
/// The public mirror of the crate-internal `SuppressionSource`;
/// exposed on [`SuppressionMarker`] so an audit can flag Lizard-style
/// markers that projects may want to migrate to the native `bca:`
/// dialect over time.
// New tool dialects beyond Native and Lizard are plausible (other
// linters with their own forgive-marker syntax), so this carries
// `#[non_exhaustive]` to keep such additions additive rather than a 2.0
// break. The CLI `marker_label` tuple match has a `_ =>` arm for the
// not-yet-known dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SuppressionDialect {
    /// Native `bca:` marker.
    Native,
    /// Lizard compatibility marker (`#lizard forgives`).
    Lizard,
}

impl From<SuppressionSource> for SuppressionDialect {
    fn from(source: SuppressionSource) -> Self {
        match source {
            SuppressionSource::Native => Self::Native,
            SuppressionSource::Lizard => Self::Lizard,
        }
    }
}

/// A single in-source suppression marker located within a file, carrying
/// the context needed to audit it.
///
/// Produced by [`crate::Ast::suppressions`] for the `bca exemptions`
/// report (issue #386). Unlike the
/// merged [`crate::FuncSpace::suppressed`] scope — which records only
/// *what* a function ends up suppressing — this records each marker's
/// own location, dialect, and the enclosing function it was written in,
/// so reviewers can see every silencer in the tree, not just its net
/// effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuppressionMarker {
    /// 1-based line of the comment that carries the marker.
    pub line: usize,
    /// Whether the marker is function- or file-scoped.
    pub target: SuppressionTarget,
    /// Which metrics the marker covers (`all` or a named set).
    pub scope: SuppressionScope,
    /// Native vs Lizard dialect.
    pub dialect: SuppressionDialect,
    /// Enclosing function name for a function-scoped marker, if the
    /// marker sits inside a function body. `None` for file-scoped
    /// markers (whole-file by definition) and for function-scoped
    /// markers written outside any function (which silence nothing — a
    /// dead marker worth surfacing in an audit).
    pub function: Option<String>,
}

/// Collect every in-source suppression marker in a parsed file, with the
/// location and enclosing-function context the `bca exemptions` audit
/// reports (issue #386).
///
/// The walk mirrors the comment-scanning step in
/// [`crate::analyze`] / [`crate::Ast::metrics`]: it visits comment nodes,
/// parses each through [`parse_marker`], and records the successes.
/// Malformed native markers are skipped silently here — the audit is a
/// read-only listing of what *is* a marker, and the threshold walk is
/// the surface that already warns on malformed bodies.
///
/// Enclosing-function attribution tracks the syntactically nearest
/// function ancestor during a depth-first walk, matching the body-
/// containment rule the real suppression logic uses (issue #289) rather
/// than line-range guessing. Markers are returned sorted by line.
///
/// Crate-internal walk core reached through the
/// [`crate::Ast::suppressions`] seam.
#[must_use]
pub(crate) fn suppression_markers<T: ParserTrait>(parser: &T) -> Vec<SuppressionMarker> {
    let code = parser.code();
    let mut markers = Vec::new();
    // Explicit-stack DFS (not recursion) so a pathologically deep AST
    // cannot overflow the call stack. Each frame carries the nearest
    // enclosing function name, borrowed from `code`, so child nodes
    // inherit it without re-deriving.
    let mut stack: Vec<(Node<'_>, Option<&str>)> = vec![(parser.root(), None)];
    while let Some((node, enclosing)) = stack.pop() {
        if T::Checker::is_comment(&node)
            && let Some(text) = node.utf8_text(code)
            && let Ok(Some(suppression)) = parse_marker(text)
        {
            // File-scoped markers are whole-file by definition, so the
            // enclosing function is irrelevant; report `None` to avoid a
            // misleading "inside fn X" attribution.
            let function = match suppression.kind {
                SuppressionKind::Function => enclosing.map(str::to_owned),
                SuppressionKind::File => None,
            };
            markers.push(SuppressionMarker {
                line: node.start_row() + 1,
                target: suppression.kind.into(),
                scope: suppression.scope,
                dialect: suppression.source.into(),
                function,
            });
        }
        // `is_func_with_code` rather than `is_func`: C/C++ identify
        // functions only via the code-aware predicate, and the default
        // impl delegates to `is_func` for every other language.
        let child_enclosing = if T::Checker::is_func_with_code(&node, code) {
            T::Getter::get_func_name(&node, code).or(enclosing)
        } else {
            enclosing
        };
        for child in node.children() {
            stack.push((child, child_enclosing));
        }
    }
    markers.sort_by_key(|m| m.line);
    markers
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
        assert!(metrics.contains(&Metric::Cyclomatic));
        assert!(metrics.contains(&Metric::Cognitive));
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn native_mixed_valid_and_unknown_metric_voids_whole_marker() {
        // The void-on-typo contract: a marker listing one valid metric
        // beside an unknown one must reject the ENTIRE list, not silently
        // honor the valid part. Otherwise a misspelled metric would still
        // suppress the correctly-spelled one beside it — the most
        // dangerous failure mode, since it widens scope on a typo. Every
        // other test feeds a marker whose only metric is unknown, where
        // "void whole marker" and "skip the unknown token" are
        // indistinguishable; only a mixed list separates them. Swapping
        // the `?` in `parse_metric_list` for `continue` makes this parse
        // to `Some({Cyclomatic})` and trips the assertion (#948).
        let err = parse_marker("// bca: suppress(cyclomatic, no_such_metric)").unwrap_err();
        assert!(
            matches!(&err, SuppressionError::UnknownMetric(name) if name == "no_such_metric"),
            "mixed valid+unknown marker must error on the unknown token, \
             voiding the whole list; got {err:?}",
        );
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
        assert!(metrics.contains(&Metric::Halstead));
        assert!(metrics.contains(&Metric::Loc));
    }

    #[test]
    fn native_unknown_metric_errors() {
        let err = parse_marker("// bca: suppress(no_such_metric)").unwrap_err();
        assert!(matches!(err, SuppressionError::UnknownMetric(_)));
        // The error must mention what was unknown so authors can
        // diagnose typos without reading our source. This is the #554
        // acceptance: the offending token is surfaced (it now flows out
        // of `Metric::from_str`'s `ParseMetricError`, not a `()` error).
        let rendered = err.to_string();
        assert!(rendered.contains("no_such_metric"));
        // And it must list the known metrics so a fix is one
        // copy-paste away.
        assert!(rendered.contains("cyclomatic"));
        // The non-suppressible `tokens` must NOT appear in the hint —
        // suggesting it would be misleading.
        assert!(
            !rendered.contains("tokens"),
            "hint must omit the non-suppressible `tokens`; got: {rendered}",
        );
        // The hint must be derived from `Metric::suppressible()` (the
        // documented single source of truth), not a re-derived list.
        // Guards #805: every suppressible metric appears, alphabetised.
        let mut expected: Vec<String> = Metric::suppressible()
            .map(|metric| metric.to_string())
            .collect();
        expected.sort_unstable();
        assert!(
            rendered.ends_with(&format!("known metrics: {}", expected.join(", "))),
            "hint must list exactly the suppressible metrics from \
             `Metric::suppressible()`, alphabetised; got: {rendered}",
        );
    }

    #[test]
    fn native_tokens_is_not_suppressible() {
        // `tokens` parses as a real `Metric` but has no threshold, so a
        // marker naming it is rejected with a distinct, actionable error
        // rather than silently accepted as a no-op suppression.
        let err = parse_marker("// bca: suppress(tokens)").unwrap_err();
        assert!(
            matches!(&err, SuppressionError::NonSuppressibleMetric(m) if m == "tokens"),
            "expected NonSuppressibleMetric(\"tokens\"); got: {err:?}",
        );
        let rendered = err.to_string();
        assert!(rendered.contains("tokens"));
        assert!(
            rendered.contains("no threshold"),
            "message must explain why tokens cannot be suppressed; got: {rendered}",
        );
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
        assert!(!s.scope.covers(Metric::Cyclomatic));
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
    fn scope_merge_all_absorbs() {
        let mut a = SuppressionScope::Some(BTreeSet::from([Metric::Loc]));
        a.merge(&SuppressionScope::All);
        assert!(a.is_all());

        let mut b = SuppressionScope::All;
        b.merge(&SuppressionScope::Some(BTreeSet::from([Metric::Loc])));
        assert!(b.is_all());
    }

    #[test]
    fn scope_merge_some_unions() {
        let mut a = SuppressionScope::Some(BTreeSet::from([Metric::Loc]));
        a.merge(&SuppressionScope::Some(BTreeSet::from([Metric::Cognitive])));
        assert!(a.covers(Metric::Loc));
        assert!(a.covers(Metric::Cognitive));
        assert!(!a.covers(Metric::Cyclomatic));
    }

    #[test]
    fn scope_covers_respects_all_vs_some() {
        assert!(SuppressionScope::All.covers(Metric::Cyclomatic));
        let some = SuppressionScope::Some(BTreeSet::from([Metric::Loc]));
        assert!(some.covers(Metric::Loc));
        assert!(!some.covers(Metric::Cyclomatic));
    }

    #[test]
    fn scope_serialization_uses_canonical_names_and_stable_order() {
        // The serialized `Some` scope must (a) spell metrics with their
        // canonical names — `nexits`, not `n_exits` or the legacy `exit`
        // — and (b) iterate in deterministic `Ord` (declaration) order so
        // snapshots are stable. Insert in scrambled order to prove the
        // ordering comes from `BTreeSet<Metric>`, not insertion order.
        let scope = SuppressionScope::Some(BTreeSet::from([
            Metric::Wmc,
            Metric::Nexits,
            Metric::Nargs,
            Metric::Cognitive,
        ]));
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"some","metrics":["cognitive","nargs","nexits","wmc"]}"#,
        );
        // Round-trips back to the same scope.
        let back: SuppressionScope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scope);
    }

    #[test]
    fn for_threshold_name_maps_dotted_subnames_to_families() {
        // Cyclomatic.modified and cyclomatic both fall under
        // Metric::Cyclomatic — silencing `cyclomatic` covers the
        // modified variant too. Same for halstead.* and loc.*.
        assert_eq!(
            threshold_metric_for_name("cyclomatic"),
            Some(Metric::Cyclomatic)
        );
        assert_eq!(
            threshold_metric_for_name("cyclomatic.modified"),
            Some(Metric::Cyclomatic)
        );
        assert_eq!(
            threshold_metric_for_name("halstead.volume"),
            Some(Metric::Halstead)
        );
        assert_eq!(threshold_metric_for_name("loc.lloc"), Some(Metric::Loc));
    }

    #[test]
    fn for_threshold_name_resolves_nexits_canonically() {
        // Post-#555 the suppression vocabulary uses the same canonical
        // `nexits` spelling as the threshold engine — no `exit` alias
        // bridge. `bca: suppress(nexits)` silences a `nexits` threshold
        // violation directly.
        assert_eq!(threshold_metric_for_name("nexits"), Some(Metric::Nexits));
    }

    #[test]
    fn for_threshold_name_returns_none_for_unknown() {
        // `tokens` is in the threshold registry but is non-suppressible
        // (no configurable threshold). Treat as "no metric family" so a
        // marker can't silence the threshold; this mirrors the parse-side
        // rejection of `bca: suppress(tokens)`.
        assert_eq!(threshold_metric_for_name("tokens"), None);
        assert_eq!(threshold_metric_for_name("no_such_metric"), None);
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

    use crate::{CppParser, ElixirParser, PythonParser, RustParser};
    use std::path::PathBuf;

    /// Collect markers from a Rust snippet via the public collector.
    fn rust_markers(src: &str) -> Vec<SuppressionMarker> {
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.rs"), None);
        suppression_markers(&parser)
    }

    #[test]
    fn collector_function_scoped_native_marker_attributes_enclosing_fn() {
        // The marker sits inside `do_thing`'s body, so the audit must
        // attribute it to that function — the body-containment rule, not
        // a line-range guess.
        let src = "fn do_thing() {\n    // bca: suppress\n    let x = 1;\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        let m = &markers[0];
        assert_eq!(m.line, 2);
        assert_eq!(m.target, SuppressionTarget::Function);
        assert_eq!(m.dialect, SuppressionDialect::Native);
        assert!(matches!(m.scope, SuppressionScope::All));
        assert_eq!(m.function.as_deref(), Some("do_thing"));
    }

    #[test]
    fn collector_metric_list_scope_is_preserved() {
        let src = "fn f() {\n    // bca: suppress(cyclomatic, cognitive)\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        let SuppressionScope::Some(metrics) = &markers[0].scope else {
            panic!("expected an explicit metric set");
        };
        assert!(metrics.contains(&Metric::Cyclomatic));
        assert!(metrics.contains(&Metric::Cognitive));
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn collector_file_scoped_marker_has_no_enclosing_fn() {
        // A `suppress-file` marker is whole-file by definition; the
        // enclosing function must be elided even though it is written
        // inside a function body.
        let src = "fn f() {\n    // bca: suppress-file\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].target, SuppressionTarget::File);
        assert_eq!(markers[0].function, None);
    }

    #[test]
    fn collector_nested_fn_attributes_innermost() {
        // The marker is inside the inner function; attribution must pick
        // the syntactically nearest enclosing function, not the outer.
        let src = "fn outer() {\n    fn inner() {\n        // bca: suppress\n    }\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].function.as_deref(), Some("inner"));
    }

    #[test]
    fn collector_marker_outside_any_fn_has_no_enclosing_fn() {
        // A function-scoped marker with no enclosing function silences
        // nothing; the audit still lists it (a dead marker) with no
        // function attribution.
        let src = "// bca: suppress\nfn f() {}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].target, SuppressionTarget::Function);
        assert_eq!(markers[0].function, None);
    }

    #[test]
    fn collector_recognizes_lizard_dialect() {
        let src = "fn f() {\n    // #lizard forgives\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].dialect, SuppressionDialect::Lizard);
        assert_eq!(markers[0].function.as_deref(), Some("f"));
    }

    #[test]
    fn collector_markers_sorted_by_line() {
        let src = "fn a() {\n    // bca: suppress\n}\nfn b() {\n    // bca: suppress\n}\n";
        let markers = rust_markers(src);
        assert_eq!(markers.len(), 2);
        assert!(markers[0].line < markers[1].line);
        assert_eq!(markers[0].function.as_deref(), Some("a"));
        assert_eq!(markers[1].function.as_deref(), Some("b"));
    }

    #[test]
    fn collector_python_hash_marker() {
        let src = "def helper():\n    # bca: suppress\n    pass\n";
        let parser = PythonParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.py"), None);
        let markers = suppression_markers(&parser);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].target, SuppressionTarget::Function);
        assert_eq!(markers[0].function.as_deref(), Some("helper"));
    }

    #[test]
    fn collector_cpp_attributes_enclosing_function() {
        // Cross-language coverage: C++ functions are detected and the
        // marker is attributed to the enclosing function.
        let src = "int compute(int a) {\n    // bca: suppress\n    return a;\n}\n";
        let parser = CppParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.cpp"), None);
        let markers = suppression_markers(&parser);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].target, SuppressionTarget::Function);
        assert_eq!(markers[0].function.as_deref(), Some("compute"));
    }

    #[test]
    fn collector_elixir_requires_code_aware_func_predicate() {
        // Elixir is the language whose `Checker::is_func` returns `false`
        // unconditionally — it identifies functions only through the
        // code-aware `is_func_with_code`. This test fails if the walk
        // reverts to plain `is_func` (the enclosing function would then
        // resolve to `None`), so it pins the predicate choice in
        // `suppression_markers`.
        let src =
            "defmodule M do\n  def parse_long do\n    # bca: suppress\n    x = 1\n  end\nend\n";
        let parser = ElixirParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.ex"), None);
        let markers = suppression_markers(&parser);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].target, SuppressionTarget::Function);
        assert_eq!(markers[0].function.as_deref(), Some("parse_long"));
    }

    #[test]
    fn collector_empty_source_yields_no_markers() {
        assert!(rust_markers("").is_empty());
        assert!(rust_markers("fn f() {}\n").is_empty());
    }
}
