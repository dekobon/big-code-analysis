// Entropy values here are exact closed forms (log2 of small integers,
// or 0); comparing within 1e-9 is the right tolerance, and the few
// exact-zero checks use a literal that is bit-exact.
#![allow(clippy::float_cmp)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{CochangeGraph, FileId, MAX_COCHANGE_COMMIT_FILES, node_entropy, shannon_entropy};

/// log₂(3) — the entropy of a uniform 3-way distribution, reused as the
/// expected value in several cases.
const LOG2_3: f64 = 1.584_962_500_721_156;

/// Shannon entropy of a fixed weight list — wraps the array-into-iterator
/// boilerplate so the cases below read as plain `entropy(&[…])`.
fn entropy(weights: &[f64]) -> f64 {
    shannon_entropy(weights.iter().copied())
}

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

#[test]
fn empty_and_singleton_distributions_have_zero_entropy() {
    assert_eq!(entropy(&[]), 0.0);
    assert_eq!(entropy(&[5.0]), 0.0);
    // A single non-zero weight among zeros is still a degenerate (certain)
    // distribution, and the `-0.0` from `p·log2(p)` is normalised to `0.0`.
    assert_eq!(entropy(&[0.0, 7.0, 0.0]), 0.0);
    assert!(!entropy(&[7.0]).is_sign_negative());
}

#[test]
fn uniform_distribution_entropy_is_log2_of_support() {
    // Two equal outcomes → exactly 1 bit; three → log2(3).
    assert!((entropy(&[1.0, 1.0]) - 1.0).abs() < 1e-12);
    assert!((entropy(&[2.0, 2.0, 2.0]) - LOG2_3).abs() < 1e-12);
}

#[test]
fn skewed_distribution_entropy_matches_closed_form() {
    // p = {0.75, 0.25} → -(0.75·log2 0.75 + 0.25·log2 0.25) = 0.8112781…
    let h = entropy(&[3.0, 1.0]);
    assert!((h - 0.811_278_124_459_133).abs() < 1e-12, "got {h}");
    // Entropy is maximised by the uniform distribution, so a skew must
    // sit strictly below the 2-outcome maximum of 1 bit.
    assert!(h < 1.0);
}

#[test]
fn negative_and_zero_weights_are_ignored() {
    // Defensive: a stray non-positive weight (clock-skew miscount) must
    // not poison the sum or produce a NaN.
    let h = entropy(&[1.0, 1.0, 0.0, -4.0]);
    assert!((h - 1.0).abs() < 1e-12, "got {h}");
    assert!(h.is_finite());
}

#[test]
fn single_file_commits_yield_no_cochange_edges() {
    let mut g = CochangeGraph::new();
    g.record_commit(&paths(&["a.rs"]), true);
    g.record_commit(&paths(&["b.rs"]), true);
    // No pair ever co-changed → both files report zero on both windows.
    assert_eq!(g.entropy(Path::new("a.rs")), (0.0, 0.0));
    assert_eq!(g.entropy(Path::new("b.rs")), (0.0, 0.0));
    // An unseen path is also (0, 0) — distinguishable only by context,
    // never by value.
    assert_eq!(g.entropy(Path::new("never.rs")), (0.0, 0.0));
}

#[test]
fn triangle_commit_gives_each_node_uniform_two_edges() {
    let mut g = CochangeGraph::new();
    g.record_commit(&paths(&["a.rs", "b.rs", "c.rs"]), true);
    // Each file co-changed with the other two, once each → weights {1,1}
    // → exactly 1 bit, on both windows (the commit was recent).
    for f in ["a.rs", "b.rs", "c.rs"] {
        let (long, recent) = g.entropy(Path::new(f));
        assert!((long - 1.0).abs() < 1e-12, "{f} long = {long}");
        assert!((recent - 1.0).abs() < 1e-12, "{f} recent = {recent}");
    }
}

#[test]
fn hub_has_higher_entropy_than_its_leaves() {
    let mut g = CochangeGraph::new();
    // `hub` co-changes with three different leaves across three commits;
    // each leaf only ever co-changes with the hub.
    g.record_commit(&paths(&["hub.rs", "a.rs"]), true);
    g.record_commit(&paths(&["hub.rs", "b.rs"]), true);
    g.record_commit(&paths(&["hub.rs", "c.rs"]), true);

    let (hub_long, _) = g.entropy(Path::new("hub.rs"));
    // Three equally-weighted neighbours → log2(3).
    assert!((hub_long - LOG2_3).abs() < 1e-12, "hub = {hub_long}");
    // A leaf has a single neighbour → no scatter → 0.
    assert_eq!(g.entropy(Path::new("a.rs")).0, 0.0);
    assert!(hub_long > g.entropy(Path::new("a.rs")).0);
}

#[test]
fn repeated_co_change_weights_the_edge() {
    let mut g = CochangeGraph::new();
    // a–b co-change twice, a–c once → a's edge weights are {2, 1}.
    g.record_commit(&paths(&["a.rs", "b.rs"]), true);
    g.record_commit(&paths(&["a.rs", "b.rs"]), true);
    g.record_commit(&paths(&["a.rs", "c.rs"]), true);

    let (a_long, _) = g.entropy(Path::new("a.rs"));
    // Entropy of {2, 1}: p = {2/3, 1/3} = 0.9182958… bits.
    assert!(
        (a_long - 0.918_295_834_054_490).abs() < 1e-12,
        "a = {a_long}"
    );
    // b co-changed only with a (weight 2) → single neighbour → 0.
    assert_eq!(g.entropy(Path::new("b.rs")).0, 0.0);
}

#[test]
fn recent_subgraph_excludes_long_only_commits() {
    let mut g = CochangeGraph::new();
    // Long-only: a co-changes with b and c outside the recent window.
    g.record_commit(&paths(&["a.rs", "b.rs"]), false);
    g.record_commit(&paths(&["a.rs", "c.rs"]), false);
    // Recent: a co-changes only with b.
    g.record_commit(&paths(&["a.rs", "b.rs"]), true);

    let (a_long, a_recent) = g.entropy(Path::new("a.rs"));
    // Long graph: a–b weight 2, a–c weight 1 → entropy of {2,1}.
    assert!(
        (a_long - 0.918_295_834_054_490).abs() < 1e-12,
        "long {a_long}"
    );
    // Recent graph: a–b only → single neighbour → 0.
    assert_eq!(a_recent, 0.0, "recent should see one neighbour only");
}

#[test]
fn over_wide_commits_are_excluded_from_the_graph() {
    let mut g = CochangeGraph::new();
    // A bulk import touching MAX+1 files must not create any edges.
    let wide: Vec<String> = (0..=MAX_COCHANGE_COMMIT_FILES)
        .map(|i| format!("f{i}.rs"))
        .collect();
    let wide_paths: Vec<PathBuf> = wide.iter().map(PathBuf::from).collect();
    g.record_commit(&wide_paths, true);
    // A file seen only in the over-wide commit has no edges.
    assert_eq!(g.entropy(Path::new("f0.rs")), (0.0, 0.0));

    // A commit exactly at the cap is still recorded (boundary inclusive).
    let at_cap: Vec<PathBuf> = (0..MAX_COCHANGE_COMMIT_FILES)
        .map(|i| PathBuf::from(format!("g{i}.rs")))
        .collect();
    g.record_commit(&at_cap, true);
    let (g0_long, _) = g.entropy(Path::new("g0.rs"));
    // (cap − 1) equally-weighted neighbours → log2(cap − 1) > 0.
    assert!(
        g0_long > 0.0,
        "at-cap commit should produce edges: {g0_long}"
    );
}

/// Logical-graph equivalence across interning orders: the same hub graph,
/// built by interning its neighbours in different orders, yields the same
/// co-change entropy and the expected closed-form value.
///
/// This checks that interning order does not change the *logical* graph,
/// and pins the closed form of `{3, 2, 1}`. It does **not** guard the
/// #334/#701 cross-process bit-identity contract: all three builds run in
/// one process, sharing one `RandomState` seed and the identical key set
/// `{FileId(1), FileId(2), FileId(3)}`, so their hub `HashMap`s iterate in
/// the *same* order and would sum identically even without the canonical
/// sort in `node_entropy`. The summation-order contract is guarded
/// directly by `node_entropy_sums_in_fileid_order_not_hashmap_order`.
#[test]
fn cochange_entropy_is_independent_of_edge_interning_order() {
    // A hub with three neighbours carrying distinct edge weights
    // {3, 2, 1}. Build the *same* logical graph two ways, varying the
    // order in which the neighbours are first interned (hence their
    // internal `FileId` assignment and HashMap layout).
    let build = |neighbour_order: &[&str]| {
        let mut g = CochangeGraph::new();
        // Intern the neighbours in the requested order via single-edge
        // commits, establishing distinct FileIds, then add the weighted
        // hub edges.
        let weights = [("x.rs", 3), ("y.rs", 2), ("z.rs", 1)];
        for name in neighbour_order {
            let &(_, w) = weights.iter().find(|(n, _)| n == name).expect("known");
            for _ in 0..w {
                g.record_commit(&paths(&["hub.rs", name]), true);
            }
        }
        g.entropy(Path::new("hub.rs")).0
    };

    let forward = build(&["x.rs", "y.rs", "z.rs"]);
    let reversed = build(&["z.rs", "y.rs", "x.rs"]);
    let shuffled = build(&["y.rs", "z.rs", "x.rs"]);

    // Bit-identical, not merely approximately equal — the contract is
    // exact reproducibility across summation orders.
    assert_eq!(
        forward.to_bits(),
        reversed.to_bits(),
        "entropy must be bit-identical regardless of edge interning order"
    );
    assert_eq!(forward.to_bits(), shuffled.to_bits());

    // And it equals the closed-form entropy of {3, 2, 1} (Σw = 6):
    // p = {1/2, 1/3, 1/6} → H = 1.459147917... bits.
    assert!(
        (forward - 1.459_147_917_027_245).abs() < 1e-12,
        "got {forward}"
    );
}

/// #334/#701 bit-identity contract, guarded directly: `node_entropy` must
/// sum a node's edge weights in **`FileId`-ascending** order, not raw
/// `HashMap` iteration order. The `-Σ p·log2(p)` fold is non-associative,
/// so two processes with different `RandomState` seeds would otherwise
/// diverge by ULPs between the live walk and a cache replay.
///
/// We assert the bit-exact value `node_entropy` produces against the
/// closed form computed over the canonical (`FileId`-ascending) weight
/// sequence — an order specified independently of the production output,
/// not a re-sort of it. Weights `{1, 2, 4, 8, 16}` are chosen because
/// their permutations yield three distinct `f64` bit patterns (verified):
/// the ascending sum is bit-distinct from other orders, so dropping the
/// `sort_unstable_by_key` in `node_entropy` makes the result follow the
/// seed-dependent `HashMap` order and no longer match this expected value.
#[test]
fn node_entropy_sums_in_fileid_order_not_hashmap_order() {
    // Hub node is FileId(0); its five neighbours FileId(1..=5) carry edge
    // weights 1,2,4,8,16. FileId-ascending iteration therefore visits the
    // weights in the order [1, 2, 4, 8, 16].
    let weights_in_fileid_order = [1u32, 2, 4, 8, 16];
    let mut hub_edges: HashMap<FileId, u32> = HashMap::new();
    for (i, &w) in weights_in_fileid_order.iter().enumerate() {
        // Neighbour ids 1..=5; insertion order here is irrelevant — only
        // the production sort (or its absence) decides summation order.
        hub_edges.insert(FileId(u32::try_from(i + 1).expect("small")), w);
    }
    // Adjacency vector indexed by node; only index 0 (the hub) is queried.
    let adjacency = vec![hub_edges];

    // Expected: the canonical fold over FileId-ascending weights. Derived
    // independently of `node_entropy`'s internals.
    let expected = shannon_entropy(weights_in_fileid_order.iter().map(|&w| f64::from(w)));
    assert_eq!(
        node_entropy(&adjacency, 0).to_bits(),
        expected.to_bits(),
        "node_entropy must sum edge weights in FileId-ascending order"
    );

    // Guard against a vacuous assertion: confirm the order is actually
    // observable in the float fold. A non-canonical order (here, reversed)
    // must produce *different* bits, so the equality above is load-bearing
    // and would break if the production sort were dropped for HashMap order.
    let reversed = shannon_entropy(weights_in_fileid_order.iter().rev().map(|&w| f64::from(w)));
    assert_ne!(
        expected.to_bits(),
        reversed.to_bits(),
        "summation order must be observable for this test to mean anything"
    );
}

#[test]
fn duplicate_path_within_a_commit_adds_no_self_loop() {
    let mut g = CochangeGraph::new();
    // A pathological commit listing the same path twice (plus a real
    // partner) must not create an a–a self edge.
    g.record_commit(&paths(&["a.rs", "a.rs", "b.rs"]), true);
    // a's only genuine neighbour is b → single neighbour → 0.
    assert_eq!(g.entropy(Path::new("a.rs")).0, 0.0);
    assert_eq!(g.entropy(Path::new("b.rs")).0, 0.0);
}
