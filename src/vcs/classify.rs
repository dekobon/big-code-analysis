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

static SECURITY_FIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:security|vulnerabilit(?:y|ies)|vuln|exploit|sanitiz(?:e|ation)|insecure|xss|csrf|injection|overflow|rce|disclosure|malicious|hijack|spoof)\b|CVE-\d{4}-\d+|CWE-\d+",
    )
    .expect("SECURITY_FIX pattern is valid")
});

static REVERT_SUBJECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^revert\b").expect("REVERT_SUBJECT pattern is valid"));

static ROLLBACK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\brollback\b").expect("ROLLBACK pattern is valid"));

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
        revert: REVERT_SUBJECT.is_match(subject) || ROLLBACK.is_match(&text),
    }
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
