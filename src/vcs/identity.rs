// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the hashing
// + canonicalisation helpers, plus the bot-filter ctor), not per-function
// logic complexity (cognitive/cyclomatic stay enforced).

//! Canonical author identity and bot-author detection.
//!
//! Backend-agnostic: the git backend resolves a commit signature
//! through `.mailmap` and hands the canonical `(name, email)` byte
//! pairs here. The email (lowercased) is the identity key — two commits
//! with the same canonical email count as one author even under
//! differing display names. Raw identities never leave the process;
//! `--emit-author-details` opts into a SHA-256 hash of the canonical
//! email instead of the plaintext.

use sha2::{Digest, Sha256};

use regex::Regex;

use super::error::Error;

/// A canonical author identity, keyed by lowercased email.
///
/// Falls back to the lowercased display name when the email is empty
/// (some imported histories carry name-only authors). Compared and
/// hashed by that key so author *counts* and *ownership* are stable
/// across display-name variation.
///
/// The key is normally the plaintext canonical email; a [`from_digest`]
/// identity instead holds the already-irreversible SHA-256 digest, used
/// when an identity is reconstructed from the persistent VCS cache (issue
/// #334), which never stores plaintext author keys on disk. Both forms
/// share the same equality/hashing contract — distinct-author *counts*
/// and *ownership* ratios are preserved either way because the digest is
/// injective for practical purposes — and within any one walk every
/// identity is of the same form, so the `is_digest` flag never makes two
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
    /// digest. The persistent VCS cache stores authors in this irreversible
    /// form (never plaintext), and replaying it must reproduce the same
    /// author counts, ownership, and emitted hashes as a fresh walk — so a
    /// `from_digest` identity hashes to itself.
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
    /// `--emit-author-details`. Stable across runs and irreversible, so
    /// it can be published without disclosing the underlying email. A
    /// [`from_digest`](AuthorId::from_digest) identity already *is* the
    /// digest, so it is returned unchanged (re-hashing would double-hash
    /// and diverge from a fresh walk).
    #[must_use]
    pub fn hashed(&self) -> String {
        if self.is_digest {
            return self.key.clone();
        }
        let mut hasher = Sha256::new();
        hasher.update(self.key.as_bytes());
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            // Writing to a String is infallible; the formatter never
            // errors, so the result is discarded deliberately.
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }
}

/// Matches author identities against a bot-exclusion pattern.
#[derive(Clone, Debug)]
pub struct BotFilter {
    pattern: Regex,
}

impl BotFilter {
    /// Compile a bot-exclusion pattern.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidBotPattern`] when `pattern` is not a
    /// valid regular expression.
    pub fn new(pattern: &str) -> Result<Self, Error> {
        let pattern = Regex::new(pattern).map_err(|e| Error::InvalidBotPattern(e.to_string()))?;
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
