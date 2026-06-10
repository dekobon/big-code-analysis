use super::*;
use std::path::PathBuf;

// The Display strings are user-facing diagnostics surfaced by the CLI,
// the `POST /vcs` error body, and the Python `ValueError`. Pin each
// variant's wording so a refactor cannot silently degrade them.
#[test]
fn display_covers_every_variant() {
    let cases: Vec<(Error, &str)> = vec![
        (
            Error::NotARepository(PathBuf::from("/tmp/x")),
            "not inside a supported version-control working tree",
        ),
        (
            Error::OpenRepository("corrupt".to_owned()),
            "failed to open repository: corrupt",
        ),
        (
            Error::ResolveRef {
                reference: "HEAD".to_owned(),
                reason: "unborn".to_owned(),
            },
            "failed to resolve revision",
        ),
        (
            Error::Walk("boom".to_owned()),
            "failed to walk commit history: boom",
        ),
        (Error::Diff("bad".to_owned()), "failed to compute diff: bad"),
        (
            Error::Mailmap("nope".to_owned()),
            "failed to apply .mailmap: nope",
        ),
        (
            Error::InvalidBotPattern("(".to_owned()),
            "invalid bot pattern: (",
        ),
        (
            Error::InvalidWindow("empty".to_owned()),
            "invalid time window: empty",
        ),
        (
            Error::InvalidTimestamp("xyz".to_owned()),
            "invalid timestamp: xyz",
        ),
        (
            Error::InvalidFormula("bogus".to_owned()),
            "unknown risk formula",
        ),
        (
            Error::Cache("disk full".to_owned()),
            "history cache error: disk full",
        ),
    ];
    for (err, expected) in cases {
        let rendered = err.to_string();
        assert!(
            rendered.contains(expected),
            "Display for {err:?} = {rendered:?}, expected to contain {expected:?}"
        );
    }
}

#[test]
fn not_a_repository_names_the_offending_path() {
    let err = Error::NotARepository(PathBuf::from("/tmp/not-a-repo"));
    assert!(err.to_string().contains("/tmp/not-a-repo"));
}

#[test]
fn resolve_ref_names_reference_and_reason() {
    let err = Error::ResolveRef {
        reference: "feature/x".to_owned(),
        reason: "no such ref".to_owned(),
    };
    let rendered = err.to_string();
    assert!(rendered.contains("feature/x"), "{rendered:?}");
    assert!(rendered.contains("no such ref"), "{rendered:?}");
}

#[test]
fn invalid_formula_lists_the_accepted_names() {
    let rendered = Error::InvalidFormula("bogus".to_owned()).to_string();
    assert!(rendered.contains("weighted"), "{rendered:?}");
    assert!(rendered.contains("percentile"), "{rendered:?}");
}

// Pin the client-input vs environment/backend classification of every
// variant (issue #641). The web boundary maps `is_client_input()` to
// `400`/`500`; a silent re-classification here would silently change the
// HTTP contract, so each variant is asserted explicitly. Mirrors the
// exhaustive match in `Error::is_client_input` — when a variant is added,
// `is_client_input` fails to compile (forcing a decision) and this test
// must gain a corresponding case.
#[test]
fn is_client_input_classifies_every_variant() {
    let s = || "x".to_owned();
    let client_input: Vec<Error> = vec![
        Error::NotARepository(PathBuf::from("/tmp/x")),
        Error::ResolveRef {
            reference: s(),
            reason: s(),
        },
        Error::InvalidBotPattern(s()),
        Error::InvalidWindow(s()),
        Error::InvalidTimestamp(s()),
        Error::InvalidFormula(s()),
        Error::InvalidFileTypeScope(s()),
        Error::InvalidBusFactorThreshold(s()),
        Error::InvalidTrend(s()),
        Error::InvalidDiff(s()),
    ];
    let environment: Vec<Error> = vec![
        Error::OpenRepository(s()),
        Error::Walk(s()),
        Error::Diff(s()),
        Error::Mailmap(s()),
        Error::Blame(s()),
        Error::Cache(s()),
    ];
    for err in client_input {
        assert!(err.is_client_input(), "{err:?} should be client input");
    }
    for err in environment {
        assert!(!err.is_client_input(), "{err:?} should not be client input");
    }
}
