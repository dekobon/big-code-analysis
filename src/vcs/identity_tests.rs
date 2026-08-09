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
fn empty_author_hash_key_is_rejected() {
    // An empty key provides no protection and is always a user mistake
    // (e.g. an unset environment variable expanding to ""); reject it loudly.
    assert!(matches!(
        AuthorHashKey::new(Vec::new()),
        Err(Error::InvalidAuthorHashKey(_))
    ));
    // A non-empty key is accepted.
    assert!(AuthorHashKey::new(b"secret".to_vec()).is_ok());
}

#[test]
fn author_hash_key_debug_redacts_the_secret() {
    // The key must never leak through `Options`' derived `Debug`; its own
    // `Debug` reports only that a key is set, never the bytes.
    let key = AuthorHashKey::new(b"super-secret-key".to_vec()).expect("non-empty");
    let rendered = format!("{key:?}");
    // Pin the *exact* redacted form. A `#[derive(Debug)]` would instead
    // render `AuthorHashKey { key: [115, 117, ...] }` — note it leaks the
    // bytes as decimals, not as the ASCII string, so a plain
    // `!contains("super-secret-key")` check would pass against the leaky
    // derive. Exact equality is what actually catches that regression.
    assert_eq!(rendered, "AuthorHashKey { .. }");
    assert!(!rendered.contains("super-secret-key"));
}

#[test]
fn emit_hashed_without_key_is_the_bare_digest() {
    // Default output (no key) is exactly `hashed()` — #956 must not change
    // the unkeyed emission, for SemVer and cache compatibility.
    let id = AuthorId::new(b"Ada", b"ada@example.com");
    assert_eq!(id.emit_hashed(None), id.hashed());
}

#[test]
fn emit_hashed_with_key_hardens_and_is_deterministic() {
    let email = "ada@example.com";
    let id = AuthorId::new(b"Ada", email.as_bytes());
    let key = AuthorHashKey::new(b"team-secret".to_vec()).expect("non-empty");

    let keyed = id.emit_hashed(Some(&key));
    // Still a 64-char SHA-256-width hex digest, never the plaintext email.
    assert_eq!(keyed.len(), 64);
    assert!(keyed.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!keyed.contains(email));
    // The key changes the emission: an attacker without the key sees a
    // value unrelated to the bare SHA-256 they could precompute.
    assert_ne!(keyed, id.hashed());
    // Stable across runs/reports for a fixed key (the property #334 and
    // cross-report pseudonyms rely on).
    assert_eq!(keyed, id.emit_hashed(Some(&key)));
    // A different key yields a different digest for the same author.
    let other_key = AuthorHashKey::new(b"different-secret".to_vec()).expect("non-empty");
    assert_ne!(keyed, id.emit_hashed(Some(&other_key)));
    // Distinct authors stay distinct under the same key.
    let bob = AuthorId::new(b"Bob", b"bob@example.com");
    assert_ne!(keyed, bob.emit_hashed(Some(&key)));
}

#[test]
fn keyed_emit_survives_a_cache_round_trip() {
    // The crux of the #334 reconciliation: the cache stores the *unkeyed*
    // inner digest, and the key is applied at finalization. So a fresh
    // identity and one reconstructed from its cached digest must emit the
    // SAME keyed value — otherwise replaying a cached walk under a key
    // would diverge from a fresh walk.
    let key = AuthorHashKey::new(b"team-secret".to_vec()).expect("non-empty");
    let fresh = AuthorId::new(b"Ada", b"ada@example.com");
    let restored = AuthorId::from_digest(fresh.hashed());
    assert_eq!(
        fresh.emit_hashed(Some(&key)),
        restored.emit_hashed(Some(&key))
    );
    // And the unkeyed round-trip is unchanged too.
    assert_eq!(fresh.emit_hashed(None), restored.emit_hashed(None));
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
fn bot_pattern_matching_is_case_insensitive() {
    // Regression for #1265: `DEFAULT_BOT_PATTERN`'s doc has promised
    // case-insensitive matching since the option shipped, but the filter
    // compiled with a plain `Regex`. A capitalised bot identity was
    // counted as a human author — diluting ownership and raising file
    // risk — while its lowercase twin was excluded.
    let filter = BotFilter::new(DEFAULT_BOT_PATTERN).expect("default pattern compiles");
    assert!(filter.is_bot(
        b"Dependabot[bot]",
        b"Dependabot[bot]@users.noreply.github.com"
    ));
    assert!(filter.is_bot(b"RENOVATE[BOT]", b""));
    assert!(filter.is_bot(b"", b"GitHub-Actions[Bot]@users.noreply.github.com"));
    // A user-supplied substring pattern behaves the way its author reads
    // it — this is the case the doc contract exists for.
    let custom = BotFilter::new("renovate").expect("custom pattern compiles");
    assert!(custom.is_bot(b"Renovate Bot", b"bot@renovateapp.com"));
    // A human is still not a bot under either.
    assert!(!filter.is_bot(b"Ada Lovelace", b"ada@example.com"));
    assert!(!custom.is_bot(b"Ada Lovelace", b"ada@example.com"));
}

#[test]
fn bot_pattern_can_opt_back_into_case_sensitivity() {
    // `case_insensitive(true)` sets only the *default* flag, so an inline
    // `(?-i)` restores exact matching for a user who needs it (#1265).
    let sensitive = BotFilter::new("(?-i)renovate").expect("inline flag compiles");
    assert!(sensitive.is_bot(b"renovate[bot]", b""));
    assert!(!sensitive.is_bot(b"Renovate Bot", b""));
}

#[test]
fn invalid_bot_pattern_is_rejected() {
    assert!(matches!(
        BotFilter::new("(unclosed"),
        Err(Error::InvalidBotPattern(_))
    ));
}

// The two digests below are pinned to absolute values rather than to
// properties of each other. Every other digest assertion in this file —
// and `vcs_author_hash_key_hardens_the_emitted_ids` in the CLI suite —
// is self-referential: it compares one digest against another produced
// by the same build, so it holds whatever the hash library emits.
//
// That shape is wrong for these two functions, because their output is a
// *stored* value with cross-version contracts on both sides.
// `src/vcs/cache.rs` writes unkeyed digests to disk and honours them
// across version boundaries — `CACHE_SCHEMA_VERSION` tracks the on-disk
// format, not the hash implementation, so a dependency bump does not
// invalidate an existing cache — and `AuthorId::hashed` documents the
// emitted digests as stable cross-report pseudonyms. A crypto bump that
// changed either output would break both contracts with nothing red.
//
// The expected values are derived independently, from Python's
// `hashlib` / `hmac`, so these assert conformance to SHA-256 and
// RFC 2104 rather than agreement with our own implementation:
//
//   d = hashlib.sha256(b"ada@example.com").hexdigest()
//   hmac.new(b"team-secret", d.encode(), hashlib.sha256).hexdigest()

/// SHA-256 of the canonical (trimmed, lowercased) email key.
const ADA_DIGEST: &str = "b5fc85e55755f9e0d030a10ab4429b6b2944855f9a0d60077fe832becbc41d72";

/// `HMAC-SHA256("team-secret", ADA_DIGEST)`. Note the pre-image is the
/// 64-character *hex string*, not the raw digest bytes.
const ADA_KEYED_DIGEST: &str = "a833c93dba4cf0558e69d3410d91fd7d09b3809dd22810b6c424a920df2195df";

#[test]
fn hashed_pins_the_exact_sha256_digest() {
    let ada = AuthorId::new(b"Ada Lovelace", b"Ada@Example.COM");
    assert_eq!(ada.hashed(), ADA_DIGEST);
    // The unkeyed emit path yields the same bytes, which is what lets a
    // cache written by one build replay bit-for-bit under another.
    assert_eq!(ada.emit_hashed(None), ADA_DIGEST);
}

#[test]
fn keyed_hashing_pins_the_exact_hmac_sha256_digest() {
    let ada = AuthorId::new(b"Ada Lovelace", b"Ada@Example.COM");
    let key = AuthorHashKey::new(b"team-secret".to_vec()).expect("the key is non-empty");
    assert_eq!(key.apply(ADA_DIGEST), ADA_KEYED_DIGEST);
    // `emit_hashed` composes the two: HMAC over the unkeyed hex digest.
    assert_eq!(ada.emit_hashed(Some(&key)), ADA_KEYED_DIGEST);
}
