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
