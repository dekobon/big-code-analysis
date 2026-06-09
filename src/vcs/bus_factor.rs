// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// DoA term assembly, the flat output DTOs, and the early-return guards
// summed across a dozen small helpers — per-function nexits peaks at 2),
// not per-function logic complexity (cognitive/cyclomatic stay enforced)
// — mirrors the sibling `score.rs` / `jit.rs`.

//! Directory- and repo-level **bus factor** (a.k.a. truck factor) over a
//! change-history walk, via the Avelino *Degree-of-Authorship* (DoA)
//! heuristic (issue #332).
//!
//! Where [`score`](crate::vcs::score) ranks *files* and
//! [`ownership_top_share`](crate::vcs::Stats::ownership_top_share)
//! captures concentration *within* one file, the bus factor measures
//! concentration *across a set of files*: the minimum number of
//! developers whose departure would leave more than a configurable
//! fraction of the set without a knowledgeable maintainer.
//!
//! # The Avelino DoA heuristic
//!
//! Avelino, Passos, Hora & Valente, *A Novel Approach for Estimating
//! Truck Factors* (ICPC 2016), score each developer's authorship of each
//! file with a regression fitted on a manually-validated corpus:
//!
//! ```text
//! DOA(d, f) = 3.293 + 1.098·FA + 0.164·DL − 0.321·ln(1 + AC)
//! ```
//!
//! where, for developer `d` and file `f`:
//!
//! - `FA` (*first authorship*) is `1` when `d` created `f`, else `0`;
//! - `DL` (*deliveries*) is the number of changes `d` made to `f`;
//! - `AC` (*accepted changes*) is the number of changes **other**
//!   developers made to `f`.
//!
//! A developer is an **author** (authority) of `f` when their DoA,
//! normalised by the file's maximum DoA, is at least
//! [`DOA_NORMALIZED_THRESHOLD`] (`0.75` in the paper). The truck factor
//! is then computed greedily: repeatedly remove the developer who
//! authors the most still-covered files until more than
//! `coverage_threshold` (default [`DEFAULT_COVERAGE_THRESHOLD`], `0.5`
//! per Avelino) of the files are *orphaned* (have no remaining author).
//! The number of developers removed is the bus factor.
//!
//! # Relation to the issue's restatement
//!
//! Issue #332 restates the formula as
//! `N₁·FA + N₂·ln(1+DL) + N₃·ln(1+AC)`. This module uses the paper's
//! **published, validated coefficients** (linear `DL`, not logged),
//! because the issue also pins "thresholds from the paper": the fitted
//! `0.164` `DL` coefficient is only meaningful against a linear `DL`
//! term, so logging it would mis-apply the regression. Normalisation
//! makes the score scale-free regardless, so a single prolific author
//! still dominates their file.
//!
//! # The result is ordinal-but-actionable
//!
//! Unlike the per-file `risk_score`, the bus factor is a small integer
//! with a direct reading: "this many key departures abandon the
//! subsystem". It still inherits the heuristic's caveats — a young repo,
//! or one with many single-author files, skews it downward (every file
//! has exactly one author, so one departure orphans it). Any change to
//! the formula, thresholds, or the grouping **must** bump
//! [`BUS_FACTOR_SCHEMA_VERSION`].
//!
//! # Co-authorship and windows
//!
//! `DL` counts every commit a developer *participated in* (author plus
//! `Co-authored-by` trailers), matching how
//! [`ownership_top_share`](crate::vcs::Stats) already credits edits, so a
//! co-authored commit credits each participant one delivery. `FA` and the
//! whole computation see only the history *within the long window*, so
//! "first authorship" means the earliest **observed** commit, not
//! necessarily the file's true creation (true creation is the
//! full-history follow-up, #329). Bot identities are filtered upstream,
//! before authorship ever reaches this module.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::identity::AuthorId;

/// Output-shape / algorithm version for the bus-factor aggregate. Bump on
/// any change to the formula, thresholds, grouping, or serialized fields.
pub const BUS_FACTOR_SCHEMA_VERSION: u32 = 1;

/// Avelino DoA regression intercept.
const DOA_INTERCEPT: f64 = 3.293;
/// Weight on first authorship (`FA`).
const DOA_FA_WEIGHT: f64 = 1.098;
/// Weight on deliveries (`DL`, linear).
const DOA_DL_WEIGHT: f64 = 0.164;
/// Weight on the log of accepted changes (`ln(1 + AC)`); subtracted.
const DOA_AC_WEIGHT: f64 = 0.321;

/// Normalised-DoA threshold above which a developer counts as an author
/// of a file (the paper's `0.75`). A file's top contributor always
/// reaches `1.0`, so every file with any history has at least one author.
pub const DOA_NORMALIZED_THRESHOLD: f64 = 0.75;

/// Default fraction of files that must be orphaned for the greedy removal
/// to stop — the Avelino coverage threshold (`0.5`).
pub const DEFAULT_COVERAGE_THRESHOLD: f64 = 0.5;

/// One developer's authorship inputs for a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorContribution {
    /// Canonical author identity (already bot-filtered, mailmap-resolved).
    pub author: AuthorId,
    /// Number of in-window commits this developer participated in for the
    /// file (`DL`).
    pub deliveries: u32,
    /// Whether this developer authored the file's earliest observed
    /// in-window commit (`FA`).
    pub first_authorship: bool,
}

/// One file's complete authorship, the unit the aggregate consumes.
///
/// Only files with at least one in-window contribution are represented;
/// a tracked-but-inactive file carries no authorship signal and is
/// excluded from the bus-factor denominator (its inclusion would orphan
/// trivially and skew the result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileAuthorship {
    /// Repository-relative path (drives directory grouping).
    pub path: PathBuf,
    /// Per-developer contributions to this file (non-empty).
    pub contributions: Vec<AuthorContribution>,
}

/// Bus factor for one set of files (the repo, or one directory).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GroupBusFactor {
    /// Developers whose combined departure orphans more than the coverage
    /// threshold of the group's files. `0` for an empty group.
    pub bus_factor: u32,
    /// Files considered (those with in-window authorship).
    pub files: u32,
    /// Distinct developers who author at least one file in the group.
    pub authors: u32,
    /// SHA-256-hashed identities of the removed key developers, in
    /// removal order; `Some` only under `--emit-author-details`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_author_ids: Option<Vec<String>>,
}

/// Bus factor for one directory grouping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DirectoryBusFactor {
    /// Repository-relative directory (forward-slash separated).
    pub directory: String,
    /// The directory's bus factor over every file recursively beneath it.
    #[serde(flatten)]
    pub group: GroupBusFactor,
}

/// The full bus-factor aggregate: repo-level plus per-directory.
///
/// Field order keeps the scalars before the nested tables so the report
/// serialises cleanly to TOML (values must precede tables); JSON / YAML
/// readers are order-insensitive.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BusFactor {
    /// Schema / algorithm version ([`BUS_FACTOR_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Coverage (abandonment) threshold actually applied, in `(0, 1)`.
    pub coverage_threshold: f64,
    /// Normalised-DoA authorship threshold ([`DOA_NORMALIZED_THRESHOLD`]).
    pub doa_threshold: f64,
    /// Bus factor over the whole repository.
    pub repo: GroupBusFactor,
    /// Bus factor for each top-level directory and each of its immediate
    /// subdirectories, sorted by directory path.
    pub by_directory: Vec<DirectoryBusFactor>,
}

/// The `vcs_aggregate` block: whole-walk aggregates surfaced alongside the
/// per-file `vcs` data. A wrapper so future aggregates can join the
/// bus factor without another top-level field.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VcsAggregate {
    /// Directory- and repo-level bus factor (issue #332).
    pub bus_factor: BusFactor,
}

/// Compute the bus-factor aggregate over every file's authorship.
///
/// `coverage_threshold` is clamped into the open interval `(0, 1)`
/// defensively (front ends validate it); `emit_author_details` opts the
/// hashed key-developer lists into the output.
#[must_use]
pub fn compute(
    authorship: &[FileAuthorship],
    coverage_threshold: f64,
    emit_author_details: bool,
) -> BusFactor {
    let coverage = clamp_threshold(coverage_threshold);

    // Resolve each file's authors (the per-file DoA pass) exactly once;
    // every group — the repo and each directory the file belongs to —
    // then reuses these author lists rather than re-scoring the file.
    let resolved: Vec<Vec<&AuthorId>> = authorship.iter().map(authors_of_file).collect();

    let repo_files: Vec<&[&AuthorId]> = resolved.iter().map(Vec::as_slice).collect();
    let repo = group_bus_factor(&repo_files, coverage, emit_author_details);

    let mut groups: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (idx, file) in authorship.iter().enumerate() {
        for key in directory_keys(&file.path) {
            groups.entry(key).or_default().push(idx);
        }
    }
    let mut by_directory: Vec<DirectoryBusFactor> = groups
        .into_iter()
        .filter_map(|(dir, indices)| {
            // A directory used as an output identifier must be UTF-8 (the
            // path rules forbid lossy conversion); a non-UTF-8 directory is
            // dropped from `by_directory` but still counts toward `repo`.
            let directory = path_to_forward_slash(&dir)?;
            let files: Vec<&[&AuthorId]> =
                indices.iter().map(|&i| resolved[i].as_slice()).collect();
            let group = group_bus_factor(&files, coverage, emit_author_details);
            Some(DirectoryBusFactor { directory, group })
        })
        .collect();
    by_directory.sort_by(|a, b| a.directory.cmp(&b.directory));

    BusFactor {
        schema_version: BUS_FACTOR_SCHEMA_VERSION,
        coverage_threshold: coverage,
        doa_threshold: DOA_NORMALIZED_THRESHOLD,
        repo,
        by_directory,
    }
}

/// Avelino DoA score for one contribution given the file's accepted-change
/// count (deliveries by other developers). Domain-safe: `AC ≥ 0`, so the
/// log never sees a non-positive argument.
fn doa(contribution: &AuthorContribution, accepted_changes: u32) -> f64 {
    DOA_INTERCEPT
        + DOA_FA_WEIGHT * f64::from(contribution.first_authorship)
        + DOA_DL_WEIGHT * f64::from(contribution.deliveries)
        - DOA_AC_WEIGHT * (1.0 + f64::from(accepted_changes)).ln()
}

/// One developer in a group's working set: a stable hashed id (for
/// deterministic tie-breaks and the optional key-author list) and the
/// indices of the files they author.
struct GroupAuthor {
    hashed: String,
    authored_files: Vec<usize>,
}

/// Compute the bus factor for one set of files, given each file's
/// already-resolved author list (see [`authors_of_file`]).
///
/// Developers are greedily removed (most-authored-files first, ties broken
/// by hashed id) until more than `coverage` of the files are orphaned; the
/// count removed is the bus factor.
fn group_bus_factor(files: &[&[&AuthorId]], coverage: f64, emit: bool) -> GroupBusFactor {
    let total_files = files.len();
    if total_files == 0 {
        return GroupBusFactor::default();
    }

    // Author registry for this group: canonical id → working-set index.
    let mut author_index: HashMap<&AuthorId, usize> = HashMap::new();
    let mut authors: Vec<GroupAuthor> = Vec::new();
    // Remaining (not-yet-removed) author count per file; a file is
    // orphaned when this hits zero.
    let mut remaining_authors = vec![0u32; total_files];

    for (file_idx, file_authors) in files.iter().enumerate() {
        for &author in *file_authors {
            let idx = *author_index.entry(author).or_insert_with(|| {
                authors.push(GroupAuthor {
                    hashed: author.hashed(),
                    authored_files: Vec::new(),
                });
                authors.len() - 1
            });
            authors[idx].authored_files.push(file_idx);
            remaining_authors[file_idx] += 1;
        }
    }

    let bus_factor = greedy_truck_factor(&authors, &mut remaining_authors, coverage, emit);
    GroupBusFactor {
        bus_factor: bus_factor.removed,
        files: u32::try_from(total_files).unwrap_or(u32::MAX),
        authors: u32::try_from(authors.len()).unwrap_or(u32::MAX),
        key_author_ids: bus_factor.key_authors,
    }
}

/// The developers who author `file`: those whose DoA, normalised by the
/// file's maximum DoA, clears [`DOA_NORMALIZED_THRESHOLD`].
///
/// The maximum DoA is normally positive (the intercept alone is `3.293`),
/// but a file with a colossal accepted-change count can drive every DoA
/// non-positive, where ratio normalisation is meaningless; that case
/// falls back to crediting the single highest-DoA contributor, so every
/// file with any history still has exactly one or more authors (never
/// zero, which would orphan it spuriously).
fn authors_of_file(file: &FileAuthorship) -> Vec<&AuthorId> {
    let total_deliveries: u32 = file
        .contributions
        .iter()
        .map(|c| c.deliveries)
        .fold(0u32, u32::saturating_add);
    let scored: Vec<(f64, &AuthorId)> = file
        .contributions
        .iter()
        .map(|c| {
            let accepted = total_deliveries.saturating_sub(c.deliveries);
            (doa(c, accepted), &c.author)
        })
        .collect();
    let max_doa = scored.iter().map(|&(d, _)| d).fold(f64::MIN, f64::max);

    if max_doa <= 0.0 {
        // Degenerate: pick the lone argmax (deterministic on ties via the
        // hashed id the caller orders by).
        return scored
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|&(_, author)| vec![author])
            .unwrap_or_default();
    }
    scored
        .into_iter()
        .filter(|&(d, _)| d / max_doa >= DOA_NORMALIZED_THRESHOLD)
        .map(|(_, author)| author)
        .collect()
}

/// Outcome of the greedy removal: the number of developers removed and,
/// optionally, their hashed ids in removal order.
struct TruckFactor {
    removed: u32,
    key_authors: Option<Vec<String>>,
}

/// Greedily remove the developer authoring the most still-covered files
/// (ties broken by hashed id, ascending, for determinism) until more than
/// `coverage` of the files are orphaned. Bounded by the author count, so
/// it always terminates.
fn greedy_truck_factor(
    authors: &[GroupAuthor],
    remaining_authors: &mut [u32],
    coverage: f64,
    emit: bool,
) -> TruckFactor {
    let total_files = remaining_authors.len();
    #[allow(clippy::cast_precision_loss)] // file counts never approach 2^53
    let target = coverage * total_files as f64;
    let mut orphaned = 0usize;
    let mut removed = 0u32;
    let mut removed_set = vec![false; authors.len()];
    let mut key_authors = emit.then(Vec::new);

    #[allow(clippy::cast_precision_loss)]
    while orphaned as f64 <= target {
        let Some(pick) = pick_top_author(authors, &removed_set, remaining_authors) else {
            break; // no remaining developer covers any still-covered file
        };
        removed_set[pick] = true;
        removed = removed.saturating_add(1);
        if let Some(ids) = key_authors.as_mut() {
            ids.push(authors[pick].hashed.clone());
        }
        for &file_idx in &authors[pick].authored_files {
            if let Some(count) = remaining_authors.get_mut(file_idx)
                && *count > 0
            {
                *count -= 1;
                if *count == 0 {
                    orphaned += 1;
                }
            }
        }
    }

    TruckFactor {
        removed,
        key_authors,
    }
}

/// The not-yet-removed developer authoring the most still-covered files
/// (a file is still covered while its remaining-author count is positive),
/// ties broken by ascending hashed id. `None` when no remaining developer
/// covers any still-covered file.
fn pick_top_author(
    authors: &[GroupAuthor],
    removed_set: &[bool],
    remaining_authors: &[u32],
) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None; // (covered-count, author index)
    for (idx, author) in authors.iter().enumerate() {
        if removed_set[idx] {
            continue;
        }
        let covered = author
            .authored_files
            .iter()
            .filter(|&&f| remaining_authors[f] > 0)
            .count();
        if covered == 0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((best_covered, best_idx)) => {
                covered > best_covered
                    || (covered == best_covered && authors[idx].hashed < authors[best_idx].hashed)
            }
        };
        if better {
            best = Some((covered, idx));
        }
    }
    best.map(|(_, idx)| idx)
}

/// The directory grouping keys for a file: its top-level directory
/// (depth 1) and the immediate subdirectory beneath it (depth 2), if any.
/// A root-level file (no directory component) yields no keys and so only
/// contributes to the repo-level group.
fn directory_keys(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Vec::new();
    };
    let mut components = parent.components().map(std::path::Component::as_os_str);
    let Some(first) = components.next() else {
        return Vec::new();
    };
    let mut keys = Vec::with_capacity(2);
    keys.push(PathBuf::from(first));
    if let Some(second) = components.next() {
        let mut depth2 = PathBuf::from(first);
        depth2.push(second);
        keys.push(depth2);
    }
    keys
}

/// Render a repo-relative directory as a forward-slash string for output,
/// returning `None` (so the caller drops it) for a non-UTF-8 path rather
/// than mangling an identifier — mirrors the CLI's `path_to_string`.
fn path_to_forward_slash(path: &Path) -> Option<String> {
    path.to_str()
        .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Clamp a coverage threshold into the open interval `(0, 1)`. A degenerate
/// `0` would make the first removal "exceed" the threshold (bus factor
/// always 1) and a `1` would never be exceeded (bus factor = every author);
/// the nudge keeps both extremes meaningful.
fn clamp_threshold(threshold: f64) -> f64 {
    const MIN: f64 = 1e-6;
    const MAX: f64 = 1.0 - 1e-6;
    if threshold.is_nan() {
        DEFAULT_COVERAGE_THRESHOLD
    } else {
        threshold.clamp(MIN, MAX)
    }
}

#[cfg(test)]
#[path = "bus_factor_tests.rs"]
mod tests;
