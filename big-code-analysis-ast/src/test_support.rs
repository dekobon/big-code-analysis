//! Parse-and-inspect helpers shared by this crate's unit tests and, behind
//! the `test-support` feature, by `big-code-analysis`'s metric tests.
//!
//! Kept out of any production file so the self-scan gate does not spend a
//! shipping module's metric budget on test-only code (#1066).

use crate::node::{Ancestors, Node, Tree};
use crate::traits::{LanguageInfo, ParserTrait, Search};

/// Every declared owner of `owners` was met by a fixture (`seen` is the
/// kind ids its walk emitted), except the never-emitted aliases in
/// `defensive`, which must not have been — and each of those must still
/// be declared, so dropping a defensive alias from a row fails here
/// rather than leaving the alias to the role-coverage gate (which
/// forgives a kind another row claims). The one such check for the four
/// role-table test harnesses (`roles/*_tests.rs`, `roles/doc_tests/`):
/// a declared-and-unseen alias that is *not* in `defensive` fails here,
/// which the inline copies this replaced did not all check.
pub fn owners_covered(
    seen: &std::collections::BTreeSet<u16>,
    owners: &[&[u16]],
    defensive: &[u16],
) {
    for &k in defensive {
        assert!(
            owners.iter().copied().flatten().any(|&o| o == k),
            "defensive kind {k} is no longer declared by any row"
        );
        assert!(
            !seen.contains(&k),
            "defensive kind {k} is now emitted; its row needs a look"
        );
    }
    for &k in owners.iter().copied().flatten() {
        if !defensive.contains(&k) {
            assert!(seen.contains(&k), "fixture never emits kind {k}");
        }
    }
}

/// A role-table harness's answers for one node, with its slots' names,
/// that prints as the names of the true slots (`{BRANCH, BRANCH_LEAF}`),
/// so a failing `check` names the slot that differs instead of leaving
/// its reader to count `[bool; N]` columns. The array type ties the
/// names to the slots, so a harness whose list drifts in length does not
/// compile.
#[derive(PartialEq)]
pub struct Labelled<const N: usize>(pub [bool; N], pub &'static [&'static str; N]);

impl<const N: usize> std::fmt::Debug for Labelled<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut set = f.debug_set();
        for (name, on) in self.1.iter().zip(self.0) {
            if on {
                set.entry(&format_args!("{name}"));
            }
        }
        set.finish()
    }
}

/// Declares a role-table harness's slots: a `pub(super) const` per name,
/// numbered by position, and `NAMES`, each slot's name by index, so a
/// slot and the name [`Labelled`] prints for it cannot disagree. Test-only:
/// the harnesses compile under `cfg(test)` alone, not `test-support`.
#[cfg(test)]
macro_rules! slots {
    ($($name:ident),+ $(,)?) => {
        /// Each slot's name, indexed by slot.
        pub(super) const NAMES: [&str; [$(stringify!($name)),+].len()] =
            [$(stringify!($name)),+];
        $crate::test_support::slots!(@at 0; $($name),+);
    };
    (@at $index:expr; $name:ident $(, $rest:ident)*) => {
        pub(super) const $name: usize = $index;
        $crate::test_support::slots!(@at $index + 1; $($rest),*);
    };
    (@at $index:expr;) => {};
}
#[cfg(test)]
pub(crate) use slots;

#[cfg(test)]
#[test]
fn labelled_prints_the_true_slots_by_name() {
    const NAMES: [&str; 3] = ["FIRST", "SECOND", "THIRD"];
    assert_eq!(
        format!("{:?}", Labelled([true, false, true], &NAMES)),
        "{FIRST, THIRD}"
    );
    assert_eq!(format!("{:?}", Labelled([false; 3], &NAMES)), "{}");
}

/// Visits `code`'s tree in pre-order, maintaining the ancestor chain
/// exactly as `big_code_analysis::spaces::compute::metrics_inner` does,
/// and hands each node to `check` together with that chain.
///
/// Keeping the bookkeeping identical to the walker's is the point: a
/// test that built the chain some other way would prove
/// [`Ancestors`](crate::node::Ancestors) self-consistent without proving
/// the walker feeds it the right slice. The walker lives in the other
/// crate, so nothing links the two but this sentence — a change to its
/// truncate/push discipline must be mirrored here by hand
/// (`.claude/rules/testing.md`, lesson #82).
///
/// # Panics
///
/// When `L`'s per-language Cargo feature is disabled in this build (see
/// [`ParserTrait::new`]), or when the fixture does not parse cleanly, so
/// the walk cannot be measuring error recovery by accident.
pub fn for_each_node_with_chain<L: LanguageInfo>(
    code: &[u8],
    check: impl FnMut(&Node<'_>, &[Node<'_>]),
) -> usize {
    walk_with_chain(&clean_tree::<L>(code), check)
}

/// [`for_each_node_with_chain`] for a fixture that recovers into an
/// `ERROR` on purpose: the shapes a grammar misparses (`$h{-not}`,
/// tree-sitter-perl's subscript recovery) can only be walked this way.
/// Same bookkeeping, same chain; only the clean-parse assertion is
/// dropped, so a test that reaches for this is saying its fixture
/// cannot parse cleanly, not that it did not check.
///
/// # Panics
///
/// When `L`'s per-language Cargo feature is disabled in this build (see
/// [`ParserTrait::new`]).
pub fn for_each_node_with_chain_tolerating_errors<L: LanguageInfo>(
    code: &[u8],
    check: impl FnMut(&Node<'_>, &[Node<'_>]),
) -> usize {
    walk_with_chain(&Tree::new::<L>(code), check)
}

/// [`for_each_node_with_chain`] through a production walk rather than a
/// replica of one: [`Search::act_on_node`], the walk `find` and `count`
/// run, which lends each node its chain together with the
/// [`SiblingCache`](crate::node::SiblingCache) and
/// [`FoldCache`](crate::node::FoldCache), built as `metrics_inner`
/// builds them ([`Ancestors::for_walk`] plus
/// [`Ancestors::with_siblings`]). A sibling read under a parent wider
/// than [`WIDE_PARENT`](crate::node::WIDE_PARENT) therefore takes the
/// cached binary search, and a change to how walks lend the caches
/// reaches this helper without a hand-made copy to update.
///
/// A separate helper rather than a change to the bare-chain one: a
/// sibling read through the bare chain always takes the seek path,
/// which needs checking too. A predicate that reads siblings should be
/// checked through both.
///
/// # Panics
///
/// As [`for_each_node_with_chain`].
pub fn for_each_node_with_sibling_cache<L: LanguageInfo>(
    code: &[u8],
    mut check: impl for<'t, 'c> FnMut(&Node<'t>, Ancestors<'t, 'c>),
) -> usize {
    let tree = clean_tree::<L>(code);
    let mut visited = 0;
    tree.get_root().act_on_node(&mut |node, ancestors| {
        visited += 1;
        check(node, ancestors);
    });
    visited
}

fn clean_tree<L: LanguageInfo>(code: &[u8]) -> Tree {
    let tree = Tree::new::<L>(code);
    assert!(
        !tree.get_root().has_error(),
        "fixture must parse cleanly, else the walk covers error recovery"
    );
    tree
}

/// [`for_each_node_with_sibling_cache`] for a source that may recover
/// into an `ERROR` — a corpus file — reporting whether it did beside
/// the node count, so the caller parses once and labels the walk
/// (#1700).
pub fn for_each_node_with_sibling_cache_tolerating_errors<L: LanguageInfo>(
    code: &[u8],
    mut check: impl for<'t, 'c> FnMut(&Node<'t>, Ancestors<'t, 'c>),
) -> (usize, bool) {
    let tree = Tree::new::<L>(code);
    let recovered = tree.get_root().has_error();
    let mut visited = 0;
    tree.get_root().act_on_node(&mut |node, ancestors| {
        visited += 1;
        check(node, ancestors);
    });
    (visited, recovered)
}

/// The tolerant walker reports what the clean one asserts: a source
/// that recovers into an `ERROR` is walked, labelled recovered, and
/// hands every node the same chain-bearing `Ancestors` the clean
/// walker does — pinned on the node count, which the two agree on for
/// one parse, and on a parent read through the cache. Only the
/// `#[ignore]`d corpus sweep reads it otherwise, so this is what runs
/// it under the gate.
#[cfg(all(test, feature = "rust"))]
#[test]
fn the_tolerant_walker_labels_recovery_and_lends_the_same_chain() {
    use crate::RustCode;
    let clean = b"fn f() { let x = 1; }\n";
    let broken = b"fn f() { let x = ; }\n";
    let mut parents_clean = Vec::new();
    let clean_count = for_each_node_with_sibling_cache::<RustCode>(clean, |node, ancestors| {
        parents_clean.push(ancestors.parent(node).map(|p| p.kind()));
    });
    let mut parents_tolerant = Vec::new();
    let (tolerant_count, recovered) =
        for_each_node_with_sibling_cache_tolerating_errors::<RustCode>(clean, |node, ancestors| {
            // A known chain, not `Ancestors::unknown()`: `parent` would
            // climb and answer the same either way (lesson 112), so the
            // chain's presence is asserted directly.
            assert!(
                ancestors.depth().is_some(),
                "the wrapper lends the walk's chain"
            );
            parents_tolerant.push(ancestors.parent(node).map(|p| p.kind()));
        });
    assert!(!recovered, "a clean parse is labelled clean");
    assert_eq!(tolerant_count, clean_count);
    assert_eq!(
        parents_tolerant, parents_clean,
        "the same chain under both walkers"
    );
    assert!(parents_clean.contains(&Some("let_declaration")));
    let (count, recovered) =
        for_each_node_with_sibling_cache_tolerating_errors::<RustCode>(broken, |_, _| {});
    assert!(recovered, "a parse with an ERROR is labelled recovered");
    assert!(count > 1);
}

fn walk_with_chain(tree: &Tree, mut check: impl FnMut(&Node<'_>, &[Node<'_>])) -> usize {
    let mut chain: Vec<Node<'_>> = Vec::new();
    let mut stack = vec![(tree.get_root(), 0_usize)];
    let mut visited = 0;
    while let Some((node, depth)) = stack.pop() {
        chain.truncate(depth);
        check(&node, &chain);
        visited += 1;
        chain.push(node);
        let first = stack.len();
        stack.extend(node.children().map(|child| (child, depth + 1)));
        stack[first..].reverse();
    }
    visited
}

fn output_submodule() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the ast crate sits under the workspace root")
        .join("tests/repositories/big-code-analysis-output")
}

/// Whether the output submodule is checked out. An uninitialised
/// submodule is an empty directory (or none); a populated one carries
/// the snapshot tree beside `control_flow/`. The `features` and
/// `chain-audit` CI legs run the lib tests with `submodules: false`; a
/// test whose obligation needs the whole corpus asserts this first and
/// carries the `corpus_` prefix those legs filter out by name.
///
/// # Panics
///
/// When the submodule path exists and cannot be enumerated — a
/// permission error, a regular file in its place — since that is a
/// broken checkout, not an absent one.
#[must_use]
pub fn control_flow_corpus_present() -> bool {
    let path = output_submodule();
    match std::fs::read_dir(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => panic!(
            "{}: cannot enumerate the output submodule: {e}",
            path.display()
        ),
        Ok(mut entries) => entries
            .next()
            .map(|entry| {
                entry.unwrap_or_else(|e| {
                    panic!(
                        "{}: unreadable entry in the output submodule: {e}",
                        path.display()
                    )
                });
            })
            .is_some(),
    }
}

/// The control-flow fixtures of `lang` — `control_flow/<dir>/*` in the
/// output submodule, the per-language files #1744 preserved — as
/// `(name, bytes)` through the reader production parses them with.
/// Mozcpp and Mozjs own no fixture and read the C++ and JavaScript
/// files under their own grammars; Ccomment and Preproc own no
/// extension and get `Some` of nothing whether or not the submodule
/// is present. `None` when the submodule is not checked out at all — the `features` and `chain-audit` CI legs run the lib
/// tests with `submodules: false` — so a caller can say what it did
/// not walk; a checkout that *has* the submodule but no `control_flow/`
/// or an empty directory is a failure, since that is a stale or
/// interrupted checkout (#1171), not an absent one.
///
/// # Panics
///
/// When the submodule is checked out at a revision without
/// `control_flow/`, or a directory holds no fixture.
#[must_use]
pub fn control_flow_sources(lang: crate::LANG) -> Option<Vec<(String, Vec<u8>)>> {
    use crate::LANG;
    let dir = match lang {
        LANG::Ccomment | LANG::Preproc => return Some(Vec::new()),
        LANG::Mozcpp => "cpp".to_owned(),
        LANG::Mozjs => "javascript".to_owned(),
        other => format!("{other:?}").to_lowercase(),
    };
    if !control_flow_corpus_present() {
        return None;
    }
    let root = output_submodule().join("control_flow").join(&dir);
    assert!(
        root.is_dir(),
        "{} missing; run `make worktree-setup` (the output submodule predates control_flow/)",
        root.display()
    );
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .expect("readable corpus directory")
        .map(|e| e.expect("readable corpus entry").path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "{} holds no fixture", root.display());
    Some(
        paths
            .into_iter()
            .map(|path| {
                let file = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("UTF-8 fixture name");
                let bytes = crate::tools::read_file_with_eol(&path)
                    .expect("readable fixture")
                    .expect("a fixture is not a binary file");
                (format!("control_flow/{dir}/{file}"), bytes)
            })
            .collect(),
    )
}

/// Walks `parser`'s tree and reports whether any node has `kind_id ==
/// target`.
///
/// The drift marker behind lesson 34. A passing `!ast_has_kind_id(…)`
/// proves an enum variant is unreachable at the pinned grammar, so a
/// defensive dispatch arm listing it is an explicit promise rather than
/// silent dead code; a bump that starts emitting the kind fails the
/// assertion instead of quietly changing a metric.
pub fn ast_has_kind_id<P: ParserTrait>(parser: &P, target: u16) -> bool {
    parser.root().preorder().any(|n| n.kind_id() == target)
}

/// A Tcl-family line whose argument list a walk can read only by
/// scanning it: `eval` with `width` braced words and one stray `]` among
/// them. Every word asks whether the list is readable, so a walk that
/// lends its `FoldCache` scans the list once and one that does not scans
/// it `width` times — the two answer identically, and only
/// `lang_helpers::argument_slot_scans` tells them apart (#1686). Shared
/// by one test per walk, in this crate and the root's.
///
/// The benchmark harness renders a same-named twin with a `"x"` quoted
/// word beside each braced one, so its `count --type string` probe has
/// a reading; its lists are therefore `2·width + 1` slots to this one's
/// `width + 1`. The two cannot share a definition — the bench crate
/// depends on the root, not on this crate's `test-support` — so a
/// change to the shape here must be mirrored there by hand.
#[must_use]
pub fn tcl_wide_erroring_eval(width: usize) -> String {
    let half = "{a b} ".repeat(width / 2);
    format!("eval {half}] {half}\n")
}

/// A Rust file whose `source_file` has exactly `width` children:
/// `#[cfg(test)] fn` items, two children each, plus one bare `fn` when
/// `width` is odd. The wide-parent tests size it from
/// [`WIDE_PARENT`](crate::node::WIDE_PARENT) to sit on each side of the
/// sibling cache's threshold, so the parity fill is what keeps the two
/// sides apart whatever value the constant takes. Shared by tests in
/// this crate and in the root's, and one of the root's counts
/// sibling-cache reads per pair, so it relies on an even `width` adding
/// no bare `fn`: a change to the fill is a change to all of them.
#[must_use]
pub fn rust_root_of_width(width: usize) -> String {
    format!(
        "{}{}",
        "#[cfg(test)]\nfn a() {}\n".repeat(width / 2),
        "fn z() {}\n".repeat(width % 2),
    )
}

/// Asserts that `predicate` holds for every input in `yes` and for none
/// in `no`, naming the offending input on failure.
#[track_caller]
pub fn assert_classifies(predicate: fn(&[u8]) -> bool, yes: &[&[u8]], no: &[&[u8]]) {
    for input in yes {
        assert!(predicate(input), "{}", String::from_utf8_lossy(input));
    }
    for input in no {
        assert!(!predicate(input), "{}", String::from_utf8_lossy(input));
    }
}
