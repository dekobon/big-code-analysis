// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// mailmap + trailer-parsing plumbing), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! Resolve the participant identities of a git commit.
//!
//! "Participants" are the commit author plus any `Co-authored-by:`
//! trailers, each canonicalised through the repository `.mailmap` (the
//! author) or by lowercased email (co-authors), then filtered against
//! the bot pattern. The returned list is de-duplicated, so a commit
//! co-authored by someone who is also the author counts that identity
//! once.

use std::sync::LazyLock;

use regex::bytes::Regex;

use crate::vcs::error::Error;
use crate::vcs::identity::{AuthorId, BotFilter};

// `Co-authored-by: Display Name <email@host>` trailer, matched
// case-insensitively per line over the raw message bytes (no lossy
// UTF-8 round-trip on the identity inputs).
static COAUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\s*co-authored-by:\s*(.*?)\s*<([^>]*)>\s*$")
        .expect("COAUTHOR trailer pattern is valid")
});

/// Resolves commit participants, holding the per-walk mailmap snapshot
/// and optional bot filter.
pub(crate) struct ParticipantResolver<'a> {
    mailmap: &'a gix::mailmap::Snapshot,
    bots: Option<&'a BotFilter>,
}

impl<'a> ParticipantResolver<'a> {
    /// `bots` is `Some` only when bot exclusion is enabled.
    pub(crate) fn new(mailmap: &'a gix::mailmap::Snapshot, bots: Option<&'a BotFilter>) -> Self {
        Self { mailmap, bots }
    }

    /// The de-duplicated, bot-filtered participant identities of one
    /// commit. An empty result means the commit was authored solely by
    /// filtered bot identities and should be skipped entirely.
    ///
    /// # Errors
    ///
    /// Propagates a [`Error::Walk`] if the commit author cannot be
    /// decoded.
    pub(crate) fn participants(&self, commit: &gix::Commit<'_>) -> Result<Vec<AuthorId>, Error> {
        let mut out: Vec<AuthorId> = Vec::new();

        let author = commit
            .author()
            .map_err(|e| Error::Walk(format!("decoding commit author: {e}")))?;
        // Canonicalise the author through `.mailmap`; co-authors are
        // canonicalised by lowercased email only (mailmap on trailers
        // is a refinement deferred past v1).
        let resolved = self.mailmap.resolve(author);
        self.push_if_human(&mut out, &resolved.name, &resolved.email);

        let message = commit
            .message_raw()
            .map_err(|e| Error::Walk(format!("decoding commit message: {e}")))?;
        for caps in COAUTHOR.captures_iter(message) {
            // capture groups 1 (name) and 2 (email) are guaranteed by a
            // successful match of the two-group pattern.
            let (Some(name), Some(email)) = (caps.get(1), caps.get(2)) else {
                continue;
            };
            self.push_if_human(&mut out, name.as_bytes(), email.as_bytes());
        }

        Ok(out)
    }

    /// Push a canonical identity unless it is a bot or already present.
    fn push_if_human(&self, out: &mut Vec<AuthorId>, name: &[u8], email: &[u8]) {
        if let Some(bots) = self.bots
            && bots.is_bot(name, email)
        {
            return;
        }
        let id = AuthorId::new(name, email);
        if !out.contains(&id) {
            out.push(id);
        }
    }
}
