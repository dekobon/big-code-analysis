// bca: suppress-file(halstead)
// File-level halstead is a many-fn aggregation artifact (the hashing +
// canonicalisation helpers), not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AuthorId(String);

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
        Self(key)
    }

    /// SHA-256 hex digest of the canonical key, for
    /// `--emit-author-details`. Stable across runs and irreversible, so
    /// it can be published without disclosing the underlying email.
    #[must_use]
    pub fn hashed(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.0.as_bytes());
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
