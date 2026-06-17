//! Shannon-entropy machinery shared by the two process-entropy signals
//! added in issue #330.
//!
//! Two distinct metrics are built on the same [`shannon_entropy`] core:
//!
//! - **Change entropy** (Hassan, 2009 — *Predicting Faults Using the
//!   Complexity of Code Changes*). Per commit, the entropy of the churn
//!   distribution across the files it touched measures how *scattered*
//!   that change was. Each file is then credited its History-Complexity
//!   share `pᵢ · H` of every commit it took part in (Hassan's HCM with
//!   the modification-probability attribution factor). The git backend
//!   computes the per-commit term; this module only supplies the entropy
//!   core so the math is unit-testable without a repository.
//! - **Co-change graph entropy** (arXiv 2504.18511, 2025). Files that
//!   change in the same commit are joined by a weighted edge (weight =
//!   number of shared commits). A file's co-change entropy is the Shannon
//!   entropy of its edge-weight distribution: low when it always co-changes
//!   with the same partner, high when its changes ripple across many
//!   different files. [`CochangeGraph`] accumulates the edges during the
//!   single history walk and computes the per-file value at finalisation.
//!
//! All entropies are in **bits** (base-2 logarithm), the convention in
//! both source papers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Initial-import commits touch thousands of files at once; a co-change
/// graph grows O(width²) in commit width, so commits wider than this are
/// excluded from the graph (only — their change entropy, which is O(width),
/// is still counted). Tuned per issue #330's worked example.
pub const MAX_COCHANGE_COMMIT_FILES: usize = 1000;

/// Shannon entropy (base 2, in bits) of a weight distribution.
///
/// Weights are normalised to a probability distribution `pᵢ = wᵢ / Σw`
/// and the entropy is `-Σ pᵢ·log₂(pᵢ)`. Non-positive weights contribute
/// nothing (a file with zero churn in a commit, or an absent edge). An
/// empty distribution, an all-zero distribution, and a single non-zero
/// weight all yield `0.0` — there is no uncertainty to measure.
///
/// Takes a cloneable iterator rather than a slice so callers on the
/// per-commit walk (churn distribution) and per-file finalise (edge
/// weights) feed it directly, with no intermediate `Vec`; the two passes
/// (total, then sum) clone the iterator.
#[must_use]
pub fn shannon_entropy<I>(weights: I) -> f64
where
    I: Iterator<Item = f64> + Clone,
{
    let total: f64 = weights.clone().filter(|&w| w > 0.0).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let mut entropy = 0.0;
    for w in weights {
        if w > 0.0 {
            let p = w / total;
            entropy -= p * p.log2();
        }
    }
    // `-0.0` (from a single-weight distribution, `p = 1`, `log2 = 0`)
    // normalises to `0.0` so serialized output never carries a signed zero.
    entropy + 0.0
}

/// Interned identifier for a file participating in the co-change graph.
///
/// Newtype rather than a bare `u32` so a node index can never be confused
/// with a co-change count (both are `u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

/// A sparse, undirected, weighted co-change graph built incrementally
/// across a history walk.
///
/// Paths are interned to [`FileId`]s and adjacency is stored sparsely
/// (`Vec<HashMap<FileId, u32>>` indexed by node), so memory scales with
/// the number of *observed* co-change pairs, not with files². Two graphs
/// are tracked in lock-step — the full long-window graph and the
/// recent-window subgraph — so both entropy variants come from one walk.
#[derive(Clone, Debug, Default)]
pub struct CochangeGraph {
    ids: HashMap<PathBuf, FileId>,
    long: Vec<HashMap<FileId, u32>>,
    recent: Vec<HashMap<FileId, u32>>,
}

impl CochangeGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `path`, allocating a fresh node (with empty adjacency in
    /// both graphs) the first time it is seen.
    fn intern(&mut self, path: &Path) -> FileId {
        if let Some(&id) = self.ids.get(path) {
            return id;
        }
        // `len` never exceeds the number of distinct tracked paths, far
        // below `u32::MAX`; saturating keeps the conversion total without
        // an `unwrap` on a provably-unreachable overflow.
        let id = FileId(u32::try_from(self.long.len()).unwrap_or(u32::MAX));
        self.ids.insert(path.to_path_buf(), id);
        self.long.push(HashMap::new());
        self.recent.push(HashMap::new());
        id
    }

    /// Record that every file in `paths` changed together in one commit,
    /// adding (or strengthening) an edge between each unordered pair.
    ///
    /// Commits touching more than [`MAX_COCHANGE_COMMIT_FILES`] files are
    /// skipped to bound the O(width²) edge growth of bulk imports. When
    /// `in_recent` is set, the edges are added to the recent subgraph too.
    pub fn record_commit<P: AsRef<Path>>(&mut self, paths: &[P], in_recent: bool) {
        if paths.len() < 2 || paths.len() > MAX_COCHANGE_COMMIT_FILES {
            return;
        }
        let ids: Vec<FileId> = paths.iter().map(|p| self.intern(p.as_ref())).collect();
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                if a == b {
                    continue; // defensive: duplicate path within one commit
                }
                bump(&mut self.long, a, b);
                if in_recent {
                    bump(&mut self.recent, a, b);
                }
            }
        }
    }

    /// Co-change entropy `(long, recent)` for `path`, in bits.
    ///
    /// Returns `(0.0, 0.0)` for a path that was never recorded or that
    /// only ever appeared in single-file commits — by definition it has
    /// no co-change neighbours. That zero is *computed*, not "missing":
    /// every file in a VCS walk receives a value.
    #[must_use]
    pub fn entropy(&self, path: &Path) -> (f64, f64) {
        let Some(&FileId(idx)) = self.ids.get(path) else {
            return (0.0, 0.0);
        };
        let idx = idx as usize;
        (
            node_entropy(&self.long, idx),
            node_entropy(&self.recent, idx),
        )
    }
}

/// Increment the symmetric edge weight between `a` and `b` in `adjacency`.
fn bump(adjacency: &mut [HashMap<FileId, u32>], a: FileId, b: FileId) {
    *adjacency[a.0 as usize].entry(b).or_insert(0) += 1;
    *adjacency[b.0 as usize].entry(a).or_insert(0) += 1;
}

/// Shannon entropy of one node's edge-weight distribution, or `0.0` when
/// the node has no edges in this graph (an absent node, or empty
/// adjacency — `shannon_entropy` returns `0.0` for the empty iterator).
///
/// The edge weights are summed in **`FileId`-sorted** order, not HashMap
/// iteration order. `shannon_entropy`'s `-Σ p·log2(p)` is a non-associative
/// float fold, so a HashMap's seed-dependent order would let the live walk
/// and a cache replay — separate processes with different `RandomState`
/// seeds — sum to ULP-divergent values, silently breaking the #334
/// bit-identity contract. Sorting pins one canonical summation order across
/// every process.
fn node_entropy(adjacency: &[HashMap<FileId, u32>], idx: usize) -> f64 {
    adjacency.get(idx).map_or(0.0, |edges| {
        let mut weights: Vec<(FileId, u32)> = edges.iter().map(|(&id, &w)| (id, w)).collect();
        weights.sort_unstable_by_key(|&(id, _)| id);
        shannon_entropy(weights.into_iter().map(|(_, w)| f64::from(w)))
    })
}

#[cfg(test)]
#[path = "entropy_tests.rs"]
mod tests;
