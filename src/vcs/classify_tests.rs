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
fn unrelated_message_classifies_as_nothing() {
    let class = c("Add documentation for the parser");
    assert_eq!(class, Classification::default());
}
