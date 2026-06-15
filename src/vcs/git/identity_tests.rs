//! Unit tests for trailer-block-scoped co-author detection (issue #812).
//!
//! End-to-end participant counting (author + co-author + bot filtering)
//! is exercised against real commits in `tests/vcs_history.rs`; these
//! tests pin the pure trailer-block heuristic that decides which slice of
//! a commit message the `Co-authored-by:` scan runs over.

use super::*;

/// Collect the `Co-authored-by:` emails honoured for a message, applying
/// the same trailer-block scoping `participants` uses.
fn coauthor_emails(message: &str) -> Vec<String> {
    let block = trailer_block(message.as_bytes());
    COAUTHOR
        .captures_iter(block)
        .filter_map(|caps| {
            let email = caps.get(2)?;
            Some(String::from_utf8_lossy(email.as_bytes()).into_owned())
        })
        .collect()
}

#[test]
fn coauthor_in_final_trailer_block_is_counted() {
    // A genuine trailing trailer block: the co-author must count.
    let msg = "pair work\n\nCo-authored-by: Grace Hopper <grace@example.com>\n";
    assert_eq!(coauthor_emails(msg), vec!["grace@example.com".to_string()]);
}

#[test]
fn coauthor_alongside_other_trailers_is_counted() {
    // A real `Co-authored-by:` sharing the final block with another
    // recognised trailer (`Signed-off-by:`) must still count.
    let msg = "\
fix the thing

Co-authored-by: Real Dev <real@example.com>
Signed-off-by: Ada <ada@example.com>";
    assert_eq!(coauthor_emails(msg), vec!["real@example.com".to_string()]);
}

#[test]
fn coauthor_quoted_in_body_is_not_counted() {
    // Issue #812: a `Co-authored-by:` line quoted inside an indented body
    // block (here, a pasted/reverted commit), with a genuine
    // `Signed-off-by:` trailer at the very end. The quoted Eve must NOT be
    // attributed; the final block is the lone `Signed-off-by:` line, which
    // carries no co-author.
    let msg = "\
Revert a bad merge

This reverts the merge that introduced the regression. For the record,
the original commit read:

    Add pairing helper

    Co-authored-by: Eve Quoted <eve@example.com>

Signed-off-by: Ada Lovelace <ada@example.com>";
    assert!(
        coauthor_emails(msg).is_empty(),
        "a body-quoted co-author must not be honoured"
    );
}

#[test]
fn coauthor_in_prose_last_paragraph_is_not_counted() {
    // The last paragraph is prose (no trailer-shaped lines beyond the
    // accidental `Co-authored-by:`), so it is not a trailer block at all.
    // One trailer-like line out of three is below the 25%-style ratio when
    // the surrounding prose dominates; even so, the guard is the key
    // behaviour — a prose paragraph that merely contains the phrase is not
    // honoured as a trailer block on its own.
    let msg = "\
Document the convention

We agreed that the footer should read like this:
the line Co-authored-by: Someone <someone@example.com>
is what GitHub turns into a co-author, but only in the final block.";
    assert!(
        coauthor_emails(msg).is_empty(),
        "an indented/prose co-author mention is not a trailer"
    );
}

#[test]
fn single_paragraph_trailer_only_message_is_counted() {
    // A message whose only paragraph is itself a trailer block (no body)
    // must still honour the co-author.
    let msg = "Co-authored-by: Solo <solo@example.com>";
    assert_eq!(coauthor_emails(msg), vec!["solo@example.com".to_string()]);
}

#[test]
fn ordinary_single_author_message_yields_no_coauthor() {
    let msg = "just a normal commit\n\nwith a body paragraph that explains why.";
    assert!(coauthor_emails(msg).is_empty());
}

#[test]
fn empty_message_yields_no_coauthor() {
    assert!(coauthor_emails("").is_empty());
}

/// Issue #817: `push_if_human` must drop a keyless author so distinct
/// authors are not collapsed into one phantom empty-keyed identity, while
/// still keeping ordinary named/emailed (and name-only) authors.
mod push_if_human {
    use super::*;
    use crate::vcs::identity::AuthorId;

    /// A resolver with an empty mailmap and no bot filter — the mailmap is
    /// irrelevant to `push_if_human`'s keyless-drop guard.
    fn resolver() -> ParticipantResolver<'static> {
        // The snapshot is leaked so the borrow can be `'static` for the
        // duration of a single test; the process exits immediately after.
        let snapshot: &'static gix::mailmap::Snapshot =
            Box::leak(Box::new(gix::mailmap::Snapshot::default()));
        ParticipantResolver::new(snapshot, None)
    }

    #[test]
    fn keyless_authors_are_dropped_not_collapsed() {
        let resolver = resolver();
        let mut out = Vec::new();
        // Two distinct commits, each authored by an empty/whitespace-only
        // identity. Before the fix both keyed to "" and collapsed into one
        // phantom author; now both are dropped.
        resolver.push_if_human(&mut out, b"", b"");
        resolver.push_if_human(&mut out, b"  ", b"");
        assert!(
            out.is_empty(),
            "keyless authors must not anchor any participant: {out:?}"
        );
    }

    #[test]
    fn named_and_name_only_authors_are_kept() {
        let resolver = resolver();
        let mut out = Vec::new();
        resolver.push_if_human(&mut out, b"Ada", b"ada@example.com");
        // A keyless author between real ones is dropped without disturbing
        // the kept identities.
        resolver.push_if_human(&mut out, b"", b"");
        // Name-only author (imported histories) is still supported.
        resolver.push_if_human(&mut out, b"Grace Hopper", b"");
        assert_eq!(out.len(), 2, "two real identities survive: {out:?}");
        assert!(out.contains(&AuthorId::new(b"Ada", b"ada@example.com")));
        assert!(out.contains(&AuthorId::new(b"Grace Hopper", b"")));
    }
}
