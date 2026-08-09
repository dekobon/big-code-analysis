use super::*;
use std::path::PathBuf;

// The Display strings are user-facing diagnostics surfaced by the CLI,
// the `POST /vcs` error body, and the Python `ValueError`. Pin each
// variant's wording so a refactor cannot silently degrade them.
#[test]
fn display_covers_every_variant() {
    // One case per `Error` variant. The count is asserted below because
    // nothing else notices an omission: this list sat at 16 of 17 from
    // #956 until #1245, leaving `InvalidAuthorHashKey`'s wording — the
    // `error` prose of a `400` — pinned by nothing.
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
            Error::InvalidFileTypeScope("empty".to_owned()),
            "invalid file-type scope: empty",
        ),
        (
            Error::InvalidBusFactorThreshold("1.5".to_owned()),
            "invalid bus-factor threshold: 1.5",
        ),
        (
            Error::InvalidAuthorHashKey("the key is empty".to_owned()),
            "invalid author-hash key: the key is empty",
        ),
        (
            Error::InvalidTrend("one point".to_owned()),
            "invalid trend parameters: one point",
        ),
        (
            Error::Blame("no such file".to_owned()),
            "failed to blame file: no such file",
        ),
        (
            Error::InvalidDiff("bad hunk".to_owned()),
            "invalid unified diff: bad hunk",
        ),
        (
            Error::Cache("disk full".to_owned()),
            "history cache error: disk full",
        ),
    ];
    // Completeness, for the half of the enum that can be enumerated.
    // Comparing discriminants ignores the payloads, which differ between
    // the samples and the cases below. The environment variants have no
    // generated list and stay hand-checked — nothing can enumerate them,
    // so a new one silently goes unpinned here.
    let covered: Vec<_> = cases
        .iter()
        .map(|(err, _)| std::mem::discriminant(err))
        .collect();
    for sample in Error::client_input_samples() {
        assert!(
            covered.contains(&std::mem::discriminant(&sample)),
            "client-input variant {sample:?} has no Display case; its wording \
             is the `error` prose of a 400 and nothing else pins it",
        );
    }
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

/// Size of the `client_input` group of `classify_error_variants!`.
///
/// Hand-maintained on purpose, and compared against
/// `Error::client_input_samples()` — a production-derived list — so it
/// fires when a variant *moves* between the two groups. That move is
/// otherwise invisible: it shrinks one list and grows the other, which
/// every per-variant assertion in this file survives.
///
/// There is deliberately no matching `ENVIRONMENT_COUNT`. The
/// environment cases below are a vec literal, so asserting its length
/// would compare a literal against a constant and could not fail for
/// any change to `Error`.
const CLIENT_INPUT_COUNT: usize = 11;

// Pin the client-input vs environment/backend classification of every
// variant (issue #641). The web boundary maps `is_client_input()` to
// `400`/`500`, so a silent re-classification changes the HTTP contract.
#[test]
fn is_client_input_classifies_every_variant() {
    // The client-input half comes from `client_input_samples()`, which
    // `classify_error_variants!` generates from the same list as
    // `is_client_input`'s own arms, so it cannot fall behind the enum.
    // The hand-written vec it replaces did exactly that: it held ten
    // entries from #956 until #1245, silently omitting
    // `InvalidAuthorHashKey` while claiming to cover every variant.
    //
    // The counts are the load-bearing part, and the reason they are not
    // derived. Deriving them would defeat them: *moving* a variant from
    // `client_input` to `environment` shrinks the sample set in step, so
    // every per-variant assertion below still passes while a client
    // mistake starts answering `500` — the inverse of the bug #1245
    // fixed, and just as quiet.
    let s = || "x".to_owned();
    let client_input = Error::client_input_samples();
    let environment: Vec<Error> = vec![
        Error::OpenRepository(s()),
        Error::Walk(s()),
        Error::Diff(s()),
        Error::Mailmap(s()),
        Error::Blame(s()),
        Error::Cache(s()),
    ];
    assert_eq!(
        client_input.len(),
        CLIENT_INPUT_COUNT,
        "the client-input group changed size; if that is deliberate, every \
         new variant also needs an `error_kind` token in the web crate",
    );
    for err in client_input {
        assert!(err.is_client_input(), "{err:?} should be client input");
    }
    for err in environment {
        assert!(!err.is_client_input(), "{err:?} should not be client input");
    }
}
