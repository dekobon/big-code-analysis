//! Walk-order document emission for the streaming stdout modes of
//! `bca metrics` / `bca ops` (#1303).
//!
//! `--output <FILE>` can sort because it holds every result before
//! serializing (#1244). Stdout streams: each worker writes its document
//! as it finishes, so redirecting stdout to a file gave a different byte
//! sequence on every run at `--jobs > 1`.
//!
//! The fix is a reorder buffer. Each dispatched file claims the index it
//! occupies in the walk's resolved file list, and a document is written
//! only when every earlier index has been released — so the emitted
//! order is the walk order regardless of which worker finished first.
//!
//! # Every dispatched path must release its slot
//!
//! A file that is skipped (empty, generated, unrecognized language),
//! fails to parse, or fails to read produces no document, and the drain
//! advances only as slots are released. Releasing is therefore done by
//! `dispatch::act_on_file` on *every* path out of the dispatch rather
//! than at the emit site, which only some files reach.
//! [`OrderedStdout::flush_remaining`] is the backstop: a slot that never
//! arrives at all — a panicked worker, or a dispatch that failed to hand
//! every path to one — delays its successors to the end of the run
//! instead of dropping them. It is what keeps a missed release a
//! *latency* bug rather than a lost document, so the guarantee above
//! does not rest on the release sites alone.
//!
//! # What the buffer holds
//!
//! Documents that finished while an earlier file was still being
//! analyzed. With per-file analysis times of the same order that is the
//! number of in-flight workers; it degrades toward the whole tree only
//! when one early file dominates the run, since nothing stops the other
//! workers from racing ahead. Bounding it hard would mean blocking a
//! worker until its turn came, which is only deadlock-free while the
//! runner hands paths out in dispatch order — a property of
//! `ConcurrentRunner`'s FIFO queue that nothing here could enforce, and
//! whose loss would hang the CLI rather than cost it memory.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// One dispatched file's place in the emission order, or `None` for a
/// path the walk's resolved list did not carry (see
/// [`OrderedStdout::slot`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot(Option<usize>);

/// The reorder buffer shared by every worker of one streaming-stdout
/// walk.
#[derive(Debug)]
pub(crate) struct OrderedStdout {
    /// Each resolved file's index in the walk's file list. Built once by
    /// [`new`](Self::new) and never written after the walk starts, so
    /// every worker reads it without a lock.
    slots: HashMap<PathBuf, usize>,
    pending: Mutex<Pending>,
}

/// Emission state: the index whose document is written next, and the
/// documents already rendered for later indices.
#[derive(Debug, Default)]
struct Pending {
    next: usize,
    /// Results held for indices above `next`: a rendered document, or
    /// `None` for a file that emitted nothing — the marker that releases
    /// the slot without writing.
    documents: BTreeMap<usize, Option<Vec<u8>>>,
}

impl OrderedStdout {
    /// Record the emission order: `paths` is the walk's resolved file
    /// list, in the order the runner will dispatch it.
    ///
    /// `None` when the list cannot be indexed. A duplicate path would
    /// give two dispatches one slot, so one document would overwrite the
    /// other. `expand_seed_paths` dedupes (`SeedSet::seen`), so a walk
    /// cannot produce one; if that ever changes, building no buffer at
    /// all degrades the run to the unordered streaming this replaced
    /// rather than losing a document.
    pub(crate) fn new(paths: &[PathBuf]) -> Option<Self> {
        let mut slots: HashMap<PathBuf, usize> = HashMap::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            slots.insert(path.clone(), index);
        }
        (slots.len() == paths.len()).then(|| Self {
            slots,
            pending: Mutex::default(),
        })
    }

    /// The place `path` occupies in the emission order.
    ///
    /// Called before the dispatch consumes the path, so the slot can be
    /// released afterwards without the caller keeping a copy of it.
    pub(crate) fn slot(&self, path: &Path) -> Slot {
        Slot(self.slots.get(path).copied())
    }

    /// Release `slot`, writing `document` once every earlier slot has
    /// been released — and then every consecutive document waiting
    /// behind it.
    ///
    /// `None` releases the slot without writing anything, which is what
    /// a skipped, unreadable, or unparseable file does.
    pub(crate) fn release(&self, slot: Slot, document: Option<Vec<u8>>) -> std::io::Result<()> {
        let Slot(Some(index)) = slot else {
            // No ordering to impose: write straight through, exactly as
            // the per-document stdout path did before #1303.
            return write_unordered(document);
        };
        let mut pending = self.lock_pending();
        // An index below `next` was already drained — only reachable
        // through `flush_remaining` having advanced past it — so its
        // document has nowhere left to go in order. Write it rather
        // than drop it.
        if index < pending.next {
            drop(pending);
            return write_unordered(document);
        }
        pending.documents.insert(index, document);
        // The guard is held across the writes, which is what keeps two
        // files' output from interleaving — the guarantee the
        // whole-document `stdout().lock()` used to provide.
        write_documents(&pending.take_ready())
    }

    /// Write anything still buffered, in index order, and reset.
    ///
    /// Reached only when a slot was never released — a worker that
    /// panicked mid-file, since every ordinary path out of the dispatch
    /// releases one. The documents behind it are emitted late rather
    /// than silently dropped.
    pub(crate) fn flush_remaining(&self) -> std::io::Result<()> {
        let mut pending = self.lock_pending();
        write_documents(&pending.take_all())
    }

    /// The reorder buffer, recovering a lock poisoned by a panicking
    /// worker rather than panicking in turn — which would take down a
    /// second worker for a walk that is still emitting, and is banned
    /// in non-test code besides. `Pending`'s invariant is restored by
    /// every `drain`, which runs to completion under the same guard.
    fn lock_pending(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Pending {
    /// Take every document from `next` upwards that is already
    /// buffered, in order, stopping at the first gap. `None` markers
    /// advance the counter and contribute nothing to write.
    ///
    /// Splitting "what is emittable now" from the writing keeps the
    /// ordering rule — the whole point of this module — a pure function
    /// of the buffer, so the tests below can drive it without a stdout
    /// to capture.
    fn take_ready(&mut self) -> Vec<Vec<u8>> {
        let mut ready = Vec::new();
        while let Some(document) = self.documents.remove(&self.next) {
            self.next += 1;
            ready.extend(document);
        }
        ready
    }

    /// Take everything buffered, in index order, gaps included.
    fn take_all(&mut self) -> Vec<Vec<u8>> {
        let mut all = Vec::new();
        for (index, document) in std::mem::take(&mut self.documents) {
            self.next = index + 1;
            all.extend(document);
        }
        all
    }
}

/// Write `documents` in the order given as one locked, buffered, flushed
/// stdout write, stopping at the first failure: once stdout has rejected
/// a write the rest of the run is broken too, and the buffer has already
/// advanced past them.
///
/// One lock and one buffer for the whole batch rather than one per
/// document: a release that drains several queued documents is routine
/// at `--jobs > 1`, and a release that drains nothing — the majority —
/// takes no lock at all.
fn write_documents(documents: &[Vec<u8>]) -> std::io::Result<()> {
    if documents.is_empty() {
        return Ok(());
    }
    crate::formats::write_buffered(None, |w| {
        for document in documents {
            w.write_all(document)?;
        }
        Ok(())
    })
}

/// Write `document` to stdout as it is, for the paths that carry no
/// ordering: a run without a reorder buffer, or a slot the buffer has
/// already drained past. `None` — the "emitted nothing" marker — writes
/// nothing.
pub(crate) fn write_unordered(document: Option<Vec<u8>>) -> std::io::Result<()> {
    document.map_or(Ok(()), |document| write_document(&document))
}

/// Write one rendered document to stdout, buffered and flushed.
///
/// The flush is the load-bearing part: a `BufWriter` flushed only by
/// `Drop` discards the error it hit, which turns a full disk into a
/// silently truncated document and a zero exit status (#1115).
fn write_document(document: &[u8]) -> std::io::Result<()> {
    crate::formats::write_buffered(None, |w| w.write_all(document))
}

#[cfg(test)]
#[path = "ordered_stdout_tests.rs"]
mod tests;
