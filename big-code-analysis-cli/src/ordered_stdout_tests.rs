//! Unit coverage for the streaming-stdout reorder buffer (#1303).
//!
//! The ordering rule is exercised through [`Pending`] rather than
//! through `release`, so the assertions are about *which documents
//! become emittable and in what order* rather than about bytes a test
//! would have to capture off the process's real stdout. The end-to-end
//! byte order is pinned by
//! `tests/output/output_unification.rs::stdout_documents_are_ordered_*`.

use super::*;

/// Buffer one file's result at `index`. `Some` is a rendered document,
/// `None` the "this file emitted nothing" marker every skipped,
/// unreadable, or unparseable file releases its slot with.
fn buffer(pending: &mut Pending, index: usize, document: Option<&str>) {
    pending
        .documents
        .insert(index, document.map(|d| d.as_bytes().to_vec()));
}

/// The emittable documents, as strings, for readable assertions.
fn ready(pending: &mut Pending) -> Vec<String> {
    pending
        .take_ready()
        .into_iter()
        .map(|d| String::from_utf8(d).expect("test documents are UTF-8"))
        .collect()
}

/// The headline contract: documents completed out of order are emitted
/// in index order, not arrival order.
#[test]
fn documents_are_emitted_in_index_order_not_arrival_order() {
    let mut pending = Pending::default();

    // Index 2 arrives first and must wait; nothing is emittable yet.
    buffer(&mut pending, 2, Some("third"));
    assert!(ready(&mut pending).is_empty(), "index 2 must wait for 0..2");

    // Index 1 arrives next — still behind the gap at 0.
    buffer(&mut pending, 1, Some("second"));
    assert!(ready(&mut pending).is_empty(), "index 1 must wait for 0");

    // Index 0 closes the gap and releases all three, in order.
    buffer(&mut pending, 0, Some("first"));
    assert_eq!(ready(&mut pending), ["first", "second", "third"]);
    assert_eq!(pending.next, 3, "the counter advances past every release");
}

/// A file that produced no document — skipped, unreadable, unparseable
/// — still releases its slot, so the documents behind it drain instead
/// of stalling forever. This is the production-only deadlock the issue
/// flags: every fixture in the byte-order tests parses.
#[test]
fn an_empty_marker_releases_its_slot_without_emitting() {
    let mut pending = Pending::default();

    buffer(&mut pending, 1, Some("after the skip"));
    assert!(ready(&mut pending).is_empty(), "index 1 waits for index 0");

    // Index 0 is the skipped file: it writes nothing but must not hold
    // index 1 back.
    buffer(&mut pending, 0, None);
    assert_eq!(ready(&mut pending), ["after the skip"]);
    assert_eq!(pending.next, 2, "the skipped slot is counted, not skipped");
}

/// A run in which nothing produced a document drains to nothing at all
/// rather than to a document of zero bytes.
#[test]
fn consecutive_empty_markers_emit_nothing() {
    let mut pending = Pending::default();

    buffer(&mut pending, 0, None);
    buffer(&mut pending, 1, None);

    assert!(ready(&mut pending).is_empty());
    assert_eq!(pending.next, 2);
}

/// `take_ready` stops at the first gap: a later index that is already
/// buffered stays buffered, or the emitted order would be the arrival
/// order the whole module exists to replace.
#[test]
fn a_gap_holds_back_every_later_document() {
    let mut pending = Pending::default();

    buffer(&mut pending, 0, Some("zero"));
    buffer(&mut pending, 2, Some("two"));

    assert_eq!(ready(&mut pending), ["zero"], "index 2 waits behind 1");
    assert_eq!(pending.next, 1);
    assert_eq!(pending.documents.len(), 1, "index 2 is still buffered");

    buffer(&mut pending, 1, Some("one"));
    assert_eq!(ready(&mut pending), ["one", "two"]);
}

/// The post-walk backstop empties the buffer across a gap that will
/// never be filled — the slot a panicked worker never released — in
/// index order rather than dropping the documents behind it.
#[test]
fn take_all_drains_across_a_gap_in_index_order() {
    let mut pending = Pending::default();

    buffer(&mut pending, 1, Some("one"));
    buffer(&mut pending, 3, Some("three"));
    buffer(&mut pending, 2, None);

    assert!(ready(&mut pending).is_empty(), "index 0 never arrives");
    assert_eq!(pending.take_all(), [b"one".to_vec(), b"three".to_vec()]);
    assert!(pending.documents.is_empty(), "the buffer is emptied");
    assert_eq!(pending.next, 4);
}

/// The emission order is the walk's resolved file list, which is not
/// globally sorted: explicit file seeds are admitted in command-line
/// order ahead of each directory seed's own sorted expansion. Indexing
/// the list is what preserves that, where sorting the paths would not.
#[test]
fn slots_follow_the_resolved_list_order_not_path_order() {
    let paths: Vec<PathBuf> = ["z.rs", "a.rs", "m.rs"].iter().map(PathBuf::from).collect();

    let ordered = OrderedStdout::new(&paths).expect("three distinct paths index cleanly");

    assert_eq!(ordered.slot(Path::new("z.rs")).0, Some(0));
    assert_eq!(ordered.slot(Path::new("a.rs")).0, Some(1));
    assert_eq!(ordered.slot(Path::new("m.rs")).0, Some(2));
}

/// A path the walk never resolved has no slot, and its document is
/// written straight through instead of waiting on a drain that would
/// never reach it.
#[test]
fn an_unindexed_path_has_no_slot() {
    let ordered = OrderedStdout::new(&[PathBuf::from("a.rs")]).expect("one path indexes cleanly");

    assert_eq!(ordered.slot(Path::new("elsewhere.rs")).0, None);
}

/// Two dispatches of one path would collide on a single slot and one
/// document would overwrite the other. `SeedSet` dedupes so a walk
/// cannot produce that list; if one ever did, no buffer is built at all
/// — every document written as it finishes, as before #1303 — rather
/// than a document being lost.
#[test]
fn a_duplicated_path_disables_ordering_rather_than_losing_a_document() {
    let duplicated = PathBuf::from("a.rs");

    assert!(
        OrderedStdout::new(&[duplicated.clone(), PathBuf::from("b.rs"), duplicated]).is_none(),
        "a duplicated path must leave the run unordered rather than share a slot"
    );
}
