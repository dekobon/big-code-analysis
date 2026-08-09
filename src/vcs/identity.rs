//! Canonical author identity and bot-author detection.
//!
//! Backend-agnostic: the git backend resolves a commit signature
//! through `.mailmap` and hands the canonical `(name, email)` byte
//! pairs here. The email (lowercased) is the identity key — two commits
//! with the same canonical email count as one author even under
//! differing display names. Raw identities never leave the process;
//! `--emit-author-details` opts into a SHA-256 hash of the canonical
//! email instead of the plaintext.
//!
//! The emitted hash is a *stable pseudonym*, not anonymization. It
//! avoids putting plaintext emails in output/caches and deters casual
//! disclosure, but it is **not** cryptographically irreversible: an
//! email is low-entropy and enumerable, so an attacker holding a
//! candidate set of emails (commit histories are public) can recover the
//! mapping by hashing each candidate or via a precomputed email→hash
//! table — the same weakness that broke Gravatar's email hashing. See
//! [`AuthorId::hashed`] for the threat model.
//!
//! Users who need stronger resistance can opt into keyed hashing with a
//! secret [`AuthorHashKey`] (`--author-hash-key`, issue #956): the emitted
//! digest becomes an HMAC the attacker cannot reproduce without the key.
//! See [`AuthorId::emit_hashed`].

// `KeyInit` carries `new_from_slice`. It was reachable through `Mac`
// under hmac 0.12 / digest 0.10; the 0.13 / 0.11 trait split makes it a
// separate import.
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use regex::{Regex, RegexBuilder};

use super::error::Error;

/// A canonical author identity, keyed by lowercased email.
///
/// Falls back to the lowercased display name when the email is empty
/// (some imported histories carry name-only authors). Compared and
/// hashed by that key so author *counts* and *ownership* are stable
/// across display-name variation.
///
/// The key is normally the plaintext canonical email; a [`from_digest`]
/// identity instead holds the SHA-256 digest, used when an identity is
/// reconstructed from the persistent VCS cache (issue #334), which never
/// stores plaintext author keys on disk. Both forms share the same
/// equality/hashing contract — distinct-author *counts* and *ownership*
/// ratios are preserved either way because the digest is collision-free
/// over the author set in practice (SHA-256 of distinct emails yields
/// distinct hashes) — and within any one walk every identity is of the
/// same form, so the `is_digest` flag never makes two
/// keys-for-the-same-person compare unequal.
///
/// [`from_digest`]: AuthorId::from_digest
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AuthorId {
    /// The canonical key: the lowercased email/name, or — for a
    /// [`from_digest`](AuthorId::from_digest) identity — its SHA-256 hex.
    key: String,
    /// `true` when `key` already holds the SHA-256 digest, so
    /// [`hashed`](AuthorId::hashed) returns it verbatim rather than
    /// hashing a second time.
    is_digest: bool,
}

impl AuthorId {
    /// Build a canonical identity from raw signature bytes.
    ///
    /// Bytes are interpreted lossily as UTF-8: an identity is a map key
    /// and a hash pre-image, never re-emitted as a path, so a stray
    /// non-UTF-8 byte degrading to U+FFFD is acceptable and keeps the
    /// function total.
    #[must_use]
    pub fn new(name: &[u8], email: &[u8]) -> Self {
        let email_key = String::from_utf8_lossy(email).trim().to_lowercase();
        let key = if email_key.is_empty() {
            String::from_utf8_lossy(name).trim().to_lowercase()
        } else {
            email_key
        };
        Self {
            key,
            is_digest: false,
        }
    }

    /// Whether this identity carries a usable key.
    ///
    /// An author with neither a name nor an email trims to the empty key,
    /// which would otherwise collapse every keyless author into one
    /// phantom identity (the same `Eq`/`Hash`). Callers building a
    /// participant set drop keyless identities so they never anchor
    /// ownership or inflate edit counts (issue #817). A
    /// [`from_digest`](AuthorId::from_digest) identity is never keyless
    /// (a SHA-256 hex is non-empty).
    #[must_use]
    pub fn has_identity(&self) -> bool {
        !self.key.is_empty()
    }

    /// Reconstruct an identity from a previously-emitted SHA-256 [`hashed`]
    /// digest. The persistent VCS cache stores authors in this hashed form
    /// (never plaintext — see [`hashed`] for what that does and does not
    /// protect), and replaying it must reproduce the same author counts,
    /// ownership, and emitted hashes as a fresh walk — so a `from_digest`
    /// identity hashes to itself.
    ///
    /// [`hashed`]: AuthorId::hashed
    #[must_use]
    pub fn from_digest(digest: String) -> Self {
        Self {
            key: digest,
            is_digest: true,
        }
    }

    /// SHA-256 hex digest of the canonical key, for
    /// `--emit-author-details`. Stable across runs, so the same author
    /// carries the same pseudonym in every report and survives a cache
    /// round-trip.
    ///
    /// # Privacy: pseudonym, not anonymization
    ///
    /// The digest avoids emitting the plaintext email and deters *casual*
    /// disclosure, but it is **not** cryptographically irreversible. The
    /// pre-image is an email — low-entropy and enumerable — and commit
    /// histories are public, so an attacker with a candidate set of emails
    /// can recover the mapping by hashing each candidate or with a
    /// precomputed email→hash table (the Gravatar weakness). Treat
    /// published digests as pseudonymization that keeps plaintext emails
    /// out of output and caches, **not** as robust anonymization against a
    /// determined attacker. Hardening (a keyed HMAC / slow KDF) is tracked
    /// as a follow-up; it must be reconciled with the issue-#334
    /// cache-replay invariant that replaying reproduces identical digests.
    ///
    /// A [`from_digest`](AuthorId::from_digest) identity already *is* the
    /// digest, so it is returned unchanged (re-hashing would double-hash
    /// and diverge from a fresh walk).
    #[must_use]
    pub fn hashed(&self) -> String {
        if self.is_digest {
            return self.key.clone();
        }
        let mut hasher = Sha256::new();
        hasher.update(self.key.as_bytes());
        to_hex(&hasher.finalize())
    }

    /// The author digest emitted for `--emit-author-details`, optionally
    /// hardened with a caller-supplied [`AuthorHashKey`].
    ///
    /// Without a key this is exactly [`hashed`](Self::hashed) — the bare
    /// SHA-256 — so default output is unchanged. With a key it is
    /// `HMAC-SHA256(key, hashed_hex)`: an attacker holding a candidate set
    /// of emails can no longer recover the mapping by hashing each
    /// candidate, nor with a precomputed email→hash table (the Gravatar
    /// weakness [`hashed`](Self::hashed) documents), because computing the
    /// digest for any candidate now requires the secret key they do not
    /// hold.
    ///
    /// Keying the *inner* digest rather than the raw email is what
    /// preserves the issue-#334 cache-replay invariant: the persistent
    /// cache stores the unkeyed inner SHA-256 (a
    /// [`from_digest`](Self::from_digest) identity *is* that digest), so
    /// replaying a cached walk under any key reproduces the same emitted
    /// value as a fresh walk, and the same cached walk can be re-finalized
    /// under a different key without re-walking. The trade-off is that the
    /// on-disk cache still holds the *unkeyed* digest; it is local-only and
    /// never published, matching the cache's existing threat model.
    #[must_use]
    pub fn emit_hashed(&self, key: Option<&AuthorHashKey>) -> String {
        let base = self.hashed();
        match key {
            None => base,
            Some(key) => key.apply(&base),
        }
    }
}

/// A secret key for the opt-in keyed author-identity hashing
/// (`--author-hash-key`, issue #956).
///
/// A newtype for two reasons: it keeps the key material out of any
/// derived `Debug` (the impl below redacts it, so the secret never lands
/// in [`Options`](super::options::Options)' debug output or a log), and it
/// gives the non-empty validation one enforced home — an
/// [`AuthorHashKey`] cannot hold a zero-length key.
#[derive(Clone)]
pub struct AuthorHashKey {
    key: Vec<u8>,
}

impl AuthorHashKey {
    /// Build a key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidAuthorHashKey`] when `key` is empty — an
    /// empty key provides no protection and is always a user mistake
    /// (e.g. an unset environment variable expanding to `""`).
    pub fn new(key: Vec<u8>) -> Result<Self, Error> {
        if key.is_empty() {
            return Err(Error::InvalidAuthorHashKey("the key is empty".to_owned()));
        }
        Ok(Self { key })
    }

    /// Harden an already-computed unkeyed hex digest into its keyed
    /// `HMAC-SHA256(key, digest_hex)` form. Shared by
    /// [`AuthorId::emit_hashed`] and the bus-factor key-author list, which
    /// holds the unkeyed digest for deterministic tie-breaking and only
    /// hardens it on the way out.
    #[must_use]
    pub(crate) fn apply(&self, digest_hex: &str) -> String {
        // `new_from_slice` accepts a key of any length (HMAC pads or hashes
        // it per RFC 2104), so it is infallible here; the `expect`
        // documents that provably-unreachable invariant (AGENTS.md permits
        // it for such cases).
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts a key of any length");
        mac.update(digest_hex.as_bytes());
        to_hex(&mac.finalize().into_bytes())
    }
}

impl std::fmt::Debug for AuthorHashKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret material — report only that a key is set,
        // so it cannot leak through `Options`' derived `Debug`.
        f.debug_struct("AuthorHashKey").finish_non_exhaustive()
    }
}

/// Lowercase-hex encode bytes. Shared by [`AuthorId::hashed`] and the
/// keyed [`AuthorHashKey::apply`] so both render digests identically.
fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        // Writing to a String is infallible; the formatter never errors,
        // so the result is discarded deliberately.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Matches author identities against a bot-exclusion pattern.
#[derive(Clone, Debug)]
pub struct BotFilter {
    pattern: Regex,
}

impl BotFilter {
    /// Compile a bot-exclusion pattern.
    ///
    /// Matching is **case-insensitive**, which is what
    /// [`DEFAULT_BOT_PATTERN`](crate::vcs::DEFAULT_BOT_PATTERN) has always
    /// documented and what the substring-style patterns users write
    /// expect: `--bot-pattern renovate` matches `Renovate Bot`. The flag
    /// is only the *default* for the pattern, so an expert can restore
    /// case sensitivity for all or part of it with an inline `(?-i)` —
    /// `(?-i)Foo` does not match `foo`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidBotPattern`] when `pattern` is not a
    /// valid regular expression.
    pub fn new(pattern: &str) -> Result<Self, Error> {
        let pattern = RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .map_err(|e| Error::InvalidBotPattern(e.to_string()))?;
        Ok(Self { pattern })
    }

    /// Returns `true` when either the display name or the email matches
    /// the bot pattern. Both are checked because automation identities
    /// vary on which field carries the `[bot]` marker.
    #[must_use]
    pub fn is_bot(&self, name: &[u8], email: &[u8]) -> bool {
        let name = String::from_utf8_lossy(name);
        let email = String::from_utf8_lossy(email);
        self.pattern.is_match(&name) || self.pattern.is_match(&email)
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
