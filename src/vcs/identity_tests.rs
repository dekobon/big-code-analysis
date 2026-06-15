use super::*;
use crate::vcs::options::DEFAULT_BOT_PATTERN;

#[test]
fn same_email_different_name_is_one_identity() {
    let a = AuthorId::new(b"Ada Lovelace", b"ada@example.com");
    let b = AuthorId::new(b"A. Lovelace", b"ada@example.com");
    assert_eq!(a, b);
}

#[test]
fn email_is_case_insensitive() {
    let a = AuthorId::new(b"Ada", b"Ada@Example.COM");
    let b = AuthorId::new(b"Ada", b"ada@example.com");
    assert_eq!(a, b);
}

#[test]
fn distinct_emails_are_distinct_identities() {
    let a = AuthorId::new(b"Ada", b"ada@example.com");
    let b = AuthorId::new(b"Grace", b"grace@example.com");
    assert_ne!(a, b);
}

#[test]
fn empty_email_falls_back_to_name() {
    let a = AuthorId::new(b"Ada Lovelace", b"");
    let b = AuthorId::new(b"ada lovelace", b"");
    assert_eq!(a, b);
    // ...and is distinct from an email-keyed identity.
    assert_ne!(a, AuthorId::new(b"Ada", b"ada@example.com"));
}

#[test]
fn has_identity_is_false_only_for_empty_key() {
    // Issue #817: an author with neither name nor email trims to the empty
    // key and must be reported as keyless so callers can drop it instead
    // of collapsing every such author into one phantom identity.
    assert!(!AuthorId::new(b"", b"").has_identity());
    assert!(!AuthorId::new(b"   ", b"  ").has_identity());
    // A name-only author (imported histories) still carries a key.
    assert!(AuthorId::new(b"Ada Lovelace", b"").has_identity());
    // An email-only author carries a key.
    assert!(AuthorId::new(b"", b"ada@example.com").has_identity());
    // A digest identity is never keyless.
    assert!(AuthorId::from_digest("deadbeef".to_string()).has_identity());
}

#[test]
fn hashed_is_stable_and_avoids_plaintext_email() {
    // The digest is a stable pseudonym that keeps the plaintext email out
    // of output (it is NOT cryptographically irreversible — see #811 and
    // `AuthorId::hashed`'s privacy note). This pins what it actually
    // provides: stability, hex shape, and that the output is not the
    // plaintext email.
    let email = "ada@example.com";
    let id = AuthorId::new(b"Ada", email.as_bytes());
    let h1 = id.hashed();
    let h2 = AuthorId::new(b"different name", email.as_bytes()).hashed();
    // Hash keys off the canonical email, so the same email hashes equal
    // regardless of display name.
    assert_eq!(h1, h2);
    // SHA-256 hex is 64 chars.
    assert_eq!(h1.len(), 64);
    // The output is the digest, never the plaintext email itself.
    assert_ne!(h1, email);
    assert!(!h1.contains(email));
    // The digest depends on the canonical email: a different email
    // hashes differently (so the hash is not a constant, and pins that
    // the email — not the name — is the pre-image).
    assert_ne!(h1, AuthorId::new(b"Ada", b"grace@example.com").hashed());
}

#[test]
fn from_digest_hashes_to_itself_and_preserves_identity() {
    // The persistent cache stores `hashed()` digests, never plaintext;
    // replaying must reproduce a fresh walk's output exactly. A
    // reconstructed identity must therefore hash back to the same digest
    // (no double-hashing) and keep equality/ownership intact.
    let original = AuthorId::new(b"Ada", b"ada@example.com");
    let digest = original.hashed();
    let restored = AuthorId::from_digest(digest.clone());
    assert_eq!(restored.hashed(), digest);
    // Two reconstructions of the same digest are one identity (so author
    // counts and ownership ratios survive a cache round-trip).
    assert_eq!(restored, AuthorId::from_digest(digest));
    // Distinct people stay distinct after reconstruction.
    let other = AuthorId::new(b"Grace", b"grace@example.com").hashed();
    assert_ne!(restored, AuthorId::from_digest(other));
}

#[test]
fn default_bot_pattern_matches_known_bots() {
    let filter = BotFilter::new(DEFAULT_BOT_PATTERN).expect("default pattern compiles");
    assert!(filter.is_bot(
        b"dependabot[bot]",
        b"49699333+dependabot[bot]@users.noreply.github.com"
    ));
    assert!(filter.is_bot(b"renovate[bot]", b"renovate@whitesourcesoftware.com"));
    assert!(filter.is_bot(b"github-actions[bot]", b""));
    // A human is not a bot.
    assert!(!filter.is_bot(b"Ada Lovelace", b"ada@example.com"));
}

#[test]
fn invalid_bot_pattern_is_rejected() {
    assert!(matches!(
        BotFilter::new("(unclosed"),
        Err(Error::InvalidBotPattern(_))
    ));
}
