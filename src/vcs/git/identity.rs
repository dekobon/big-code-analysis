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

// A git "trailer" line: `Token: value`, where the token is one or more
// `[A-Za-z0-9-]` immediately followed by a colon and a separator
// (whitespace), matching git-interpret-trailers' default `: ` parsing.
// Anchored to the line start (no leading whitespace) so that an indented
// continuation/quoted line is *not* counted as a trailer.
static TRAILER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[A-Za-z0-9][A-Za-z0-9-]*:\s").expect("TRAILER_LINE pattern is valid")
});

// Minimum fraction of a candidate block's lines that must look like
// trailers for the block to be treated as a trailer block, mirroring
// git-interpret-trailers' built-in 25% threshold (`trailer.ifexists` /
// the `25` in `trailer.c`). A block below this is prose that merely
// happens to end the message, so its `Co-authored-by:`-shaped lines are
// not honoured as real trailers.
const TRAILER_BLOCK_MIN_RATIO: f64 = 0.25;

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
        // Git only honours trailers in the message's final block, so a
        // `Co-authored-by:` line quoted in the body (a pasted log, a
        // reverted commit, a code block) is not a real co-author. Scope
        // the scan to the detected trailer block; an empty slice when the
        // last paragraph is prose.
        let trailer_block = trailer_block(message);
        for caps in COAUTHOR.captures_iter(trailer_block) {
            // capture groups 1 (name) and 2 (email) are guaranteed by a
            // successful match of the two-group pattern.
            let (Some(name), Some(email)) = (caps.get(1), caps.get(2)) else {
                continue;
            };
            self.push_if_human(&mut out, name.as_bytes(), email.as_bytes());
        }

        Ok(out)
    }

    /// Push a canonical identity unless it is a bot, has no usable key,
    /// or is already present.
    fn push_if_human(&self, out: &mut Vec<AuthorId>, name: &[u8], email: &[u8]) {
        if let Some(bots) = self.bots
            && bots.is_bot(name, email)
        {
            return;
        }
        let id = AuthorId::new(name, email);
        // An identity with neither name nor email carries no identifying
        // key (its key is the empty string). Counting it would collapse
        // every such author into one phantom identity that accrues all of
        // their edits, ownership, and first-authorship — and deflates the
        // distinct-author count. Drop it, mirroring the bot guard above: a
        // commit whose only author is keyless contributes no participant,
        // and `process_commit` already treats an empty participant set as
        // an unknown-author commit to skip.
        if !id.has_identity() {
            return;
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
}

/// The byte slice of `message`'s final trailer block, or an empty slice
/// when the last paragraph is prose rather than trailers.
///
/// Mirrors `git interpret-trailers`: the trailer block is the message's
/// last paragraph (the text after the final blank-line separator),
/// honoured only when it *looks like* trailers — at least one line is a
/// recognised `Token: value` trailer and at least [`TRAILER_BLOCK_MIN_RATIO`]
/// of its non-blank lines are. This keeps a `Co-authored-by:` line quoted
/// in the commit body (a pasted log, a reverted commit, an indented code
/// block) from being mistaken for a real co-author trailer, while still
/// honouring a genuine trailing `Co-authored-by:` alongside, e.g., a
/// `Signed-off-by:` trailer.
fn trailer_block(message: &[u8]) -> &[u8] {
    // The last paragraph starts after the final run of blank lines. A
    // blank-line separator is `\n\n` (possibly with trailing whitespace on
    // the intervening line, which still leaves the bytes split on `\n`).
    // Split on `\n` and walk back to the last empty line to find the
    // paragraph boundary.
    let trimmed = trim_trailing_newlines(message);
    let block = &trimmed[last_paragraph_start(trimmed)..];

    let mut non_blank = 0_usize;
    let mut trailer_like = 0_usize;
    for line in block.split(|&b| b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        non_blank += 1;
        if TRAILER_LINE.is_match(line) {
            trailer_like += 1;
        }
    }

    // Honour the block only when at least one line is a trailer and
    // trailers make up at least the git threshold of it — otherwise it is
    // prose that merely ends the message.
    #[allow(clippy::cast_precision_loss)]
    let is_trailer_block =
        trailer_like > 0 && (trailer_like as f64) >= (non_blank as f64) * TRAILER_BLOCK_MIN_RATIO;
    if is_trailer_block { block } else { &[] }
}

/// Byte offset of the start of the last paragraph: the byte just after
/// the final blank-line separator, or `0` when the message is a single
/// paragraph.
fn last_paragraph_start(message: &[u8]) -> usize {
    // A paragraph break is a blank/whitespace-only line; the last
    // paragraph begins on the first content line after the final such
    // break. Track the byte just past each blank line's `\n` and keep the
    // last one that still leaves content after it.
    let mut offset = 0_usize;
    let mut start = 0_usize;
    for line in message.split(|&b| b == b'\n') {
        let after_line = offset + line.len() + 1;
        if line.iter().all(u8::is_ascii_whitespace) && after_line < message.len() {
            start = after_line;
        }
        offset = after_line;
    }
    start
}

/// Strip trailing `\n` / `\r` bytes so the final paragraph is not an
/// empty trailing line.
fn trim_trailing_newlines(message: &[u8]) -> &[u8] {
    let end = message
        .iter()
        .rposition(|&b| b != b'\n' && b != b'\r')
        .map_or(0, |i| i + 1);
    &message[..end]
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
