//! Commit-message classification: bug-fix, security-fix, and revert
//! detection via curated keyword regexes.
//!
//! The keyword approach follows the commit-message classification
//! literature (Pascarella/Bavota for bug-fix detection; the
//! Sentence-Level VFC studies and PySecDB for security fixes). It is a
//! coarse signal by design — full SZZ bug-inducing-commit detection is
//! explicitly out of scope for v1 (issue #328) — so the patterns favour
//! precision (word-boundary anchored, false-positive-aware) over
//! recall.

use std::sync::LazyLock;

use regex::Regex;

/// What a single commit message matched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Classification {
    /// The message matched a bug-fix keyword.
    pub bug_fix: bool,
    /// The message matched a security-fix keyword.
    pub security_fix: bool,
    /// The subject is a revert / rollback.
    pub revert: bool,
}

// Word boundaries (`\b`) keep "prefix"/"suffix" from matching `fix` and
// "insecurity" from matching `security`; the regression tests in
// `classify_tests.rs` pin exactly those false-positive cases.
//
// The patterns are compile-time constants, so the `expect` in each
// `LazyLock` initialiser guards a provably-unreachable failure (a
// malformed literal would fail the test suite's `patterns_compile`
// before any release).
static BUG_FIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:fix(?:es|ed|ing)?|bug(?:fix)?(?:es|s)?|defect|hotfix|regression|fault|crash)\b",
    )
    .expect("BUG_FIX pattern is valid")
});

// `injection` and `overflow` are bare-term false-positive magnets:
// "dependency injection", "text overflow", and arithmetic "integer
// overflow" are routine non-security commits (issue #808). The
// precision-over-recall contract requires a security-specific
// qualifier, so each is gated behind a `\s+`-joined attack-vector
// token. "integer overflow" and "stack overflow" are deliberately
// excluded: the former is an ordinary arithmetic bug far more often
// than a security finding, and the latter doubles as the website
// name — both are ambiguous, and precision wins over recall.
static SECURITY_FIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:security|vulnerabilit(?:y|ies)|vuln|exploit|sanitiz(?:e|ation)|insecure|xss|csrf|rce|disclosure|malicious|hijack|spoof|(?:sql|command|code|html|ldap|os|xml)\s+injection|(?:buffer|heap)\s+overflow)\b|CVE-\d{4}-\d+|CWE-\d+",
    )
    .expect("SECURITY_FIX pattern is valid")
});

static REVERT_SUBJECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^revert\b").expect("REVERT_SUBJECT pattern is valid"));

// Rollback gets the same subject-line precision discipline as revert
// (issue #806): a body-prose "rollback" mention must not flip the
// whole commit. `^`-anchored to mirror REVERT_SUBJECT — a leading
// "Rollback the migration" still classifies, a buried one does not.
static ROLLBACK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^rollback\b").expect("ROLLBACK pattern is valid"));

/// Classify a raw commit message.
///
/// The message is matched lossily as UTF-8 — classification is a
/// heuristic over human-readable prose, never an identifier, so a
/// non-UTF-8 byte degrading to U+FFFD cannot corrupt downstream state.
#[must_use]
pub fn classify(message: &[u8]) -> Classification {
    let text = String::from_utf8_lossy(message);
    // The subject is the first line; `^Revert ...` is git's
    // auto-generated revert subject.
    let subject = text.lines().next().unwrap_or("");
    Classification {
        bug_fix: BUG_FIX.is_match(&text),
        security_fix: SECURITY_FIX.is_match(&text),
        revert: REVERT_SUBJECT.is_match(subject) || ROLLBACK.is_match(subject),
    }
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
