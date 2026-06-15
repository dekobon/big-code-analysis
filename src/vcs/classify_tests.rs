use super::*;

fn c(message: &str) -> Classification {
    classify(message.as_bytes())
}

#[test]
fn bug_fix_keywords_match() {
    assert!(c("fix crash on startup").bug_fix);
    assert!(c("Fixed the null deref").bug_fix);
    assert!(c("fixes #1234").bug_fix);
    assert!(c("bugfix: off-by-one").bug_fix);
    assert!(c("address a regression").bug_fix);
}

#[test]
fn bug_fix_avoids_substring_false_positives() {
    // "prefix"/"suffix"/"affix" embed "fix" mid-word; word boundaries
    // must keep them from matching (the canonical false-positive case).
    assert!(!c("rename the prefix handling").bug_fix);
    assert!(!c("drop the filename suffix").bug_fix);
    assert!(!c("add a new feature").bug_fix);
}

#[test]
fn security_keywords_match() {
    assert!(c("patch the XSS hole").security_fix);
    assert!(c("Resolve CVE-2021-44228").security_fix);
    assert!(c("sanitize user input").security_fix);
    assert!(c("fix SQL injection").security_fix);
    assert!(c("security hardening").security_fix);
}

#[test]
fn security_avoids_substring_false_positives() {
    // "insecurity" embeds "security"; the boundary must reject it.
    assert!(!c("address job insecurity in docs").security_fix);
    assert!(!c("update the changelog").security_fix);
}

#[test]
fn security_injection_overflow_require_qualifier() {
    // Issue #808: bare `injection` / `overflow` matched routine
    // non-security commits. The terms are now qualifier-gated.
    assert!(!c("add dependency injection container").security_fix);
    assert!(!c("fix text overflow in sidebar").security_fix);
    // "integer overflow" / "stack overflow" are deliberately excluded
    // as ambiguous (arithmetic bug / website name): precision wins.
    assert!(!c("handle integer overflow in the counter").security_fix);
    assert!(!c("link to the stack overflow answer").security_fix);
    // Qualified attack-vector forms still classify.
    assert!(c("fix SQL injection").security_fix);
    assert!(c("buffer overflow fix").security_fix);
    assert!(c("patch heap overflow in decoder").security_fix);
    assert!(c("command injection in shell wrapper").security_fix);
}

#[test]
fn revert_detected_from_subject_or_rollback() {
    assert!(c("Revert \"add the broken feature\"").revert);
    assert!(c("revert the bad merge").revert);
    assert!(c("rollback the migration").revert);
    // The subject anchor matters: a non-leading "revert" in prose must
    // NOT classify as a revert (a de-anchored `\brevert\b` would, so
    // this pins the `^` in REVERT_SUBJECT).
    assert!(!c("we should not revert this change").revert);
    assert!(!c("this does not undo anything").revert);
}

#[test]
fn rollback_only_classifies_from_subject() {
    // Issue #806: rollback now gets revert's subject-line discipline.
    // A leading "Rollback ..." subject still classifies.
    assert!(c("Rollback the migration").revert);
    // A body-prose "rollback" mention must NOT flip the whole commit,
    // exactly as a body-prose "revert" mention does not.
    assert!(!c("feat: add retry logic\n\nThis avoids a rollback later").revert);
    assert!(!c("document the rollback procedure in the runbook").revert);
}

#[test]
fn unrelated_message_classifies_as_nothing() {
    let class = c("Add documentation for the parser");
    assert_eq!(class, Classification::default());
}
