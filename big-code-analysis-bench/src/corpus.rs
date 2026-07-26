//! A deterministic slice of the checked-out corpus submodules.
//!
//! # Why a slice, and why it reports itself
//!
//! Synthetic depth shapes are not representative of ordinary input:
//! real files measured with `bca dump` run to a mean AST depth of 8-13
//! and a maximum of 23-39, with about half the nodes being leaves.
//! Both kinds of measurement are needed, so the criterion benches run
//! over real files as well.
//!
//! The whole corpus is far too large to walk per iteration
//! (`tests/repositories/DeepSpeech` alone is 25 022 files), so this
//! takes a bounded slice. A slice invites the failure that produced a
//! wrong published number during the #1052 / #1062 work: a run was
//! described as "2862 Python files" when the tree it actually walked
//! was 78% C/C++. [`CorpusSlice::summary`] therefore prints what was
//! selected — file count, byte total, and the per-language breakdown —
//! and every bench that consumes the slice prints it before measuring.
//!
//! Selection is deterministic given the pinned submodule commits:
//! directory entries are visited in sorted order and the per-language
//! quota is filled in that order, so two runs at the same parent
//! commit select the same files.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use big_code_analysis::{LANG, get_language_for_file};

/// Corpus submodules, relative to the repository root.
///
/// Three different language mixes on purpose: `serde` is Rust,
/// `pdf.js` is JavaScript, and `DeepSpeech` is predominantly C/C++
/// with a Python layer.
pub const CORPUS_ROOTS: &[&str] = &[
    "tests/repositories/serde",
    "tests/repositories/pdf.js",
    "tests/repositories/DeepSpeech",
];

/// Directory names never descended into.
const SKIPPED_DIRS: &[&str] = &[".git", "node_modules", "target", "build", "third_party"];

/// Bounds on how much of the corpus a slice takes.
///
/// [`Limits::default`] is what the bench targets use; the type is
/// public so an operator who wants a wider slice can build one without
/// editing the crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Files taken per language.
    ///
    /// Per-language rather than global so a language with thousands of
    /// files in the corpus cannot crowd out the rest.
    pub files_per_language: usize,
    /// Smallest file admitted, in bytes. Below this a file is a
    /// licence header or a re-export stub and the measurement is all
    /// fixed per-file overhead.
    pub min_file_bytes: usize,
    /// Largest file admitted, in bytes. Generated blobs and vendored
    /// bundles run to megabytes and would dominate the total on their
    /// own, making the slice a measurement of one file.
    pub max_file_bytes: usize,
    /// Ceiling on the whole slice, in bytes.
    pub max_total_bytes: usize,
}

impl Default for Limits {
    /// Sized to keep one full metric walk in the tens of milliseconds,
    /// so criterion collects its default sample count in seconds
    /// rather than minutes.
    fn default() -> Self {
        Self {
            files_per_language: 16,
            min_file_bytes: 512,
            max_file_bytes: 64 * 1_024,
            max_total_bytes: 2 * 1_024 * 1_024,
        }
    }
}

/// One selected corpus file, with its contents already read.
pub struct CorpusFile {
    /// Absolute path to the file, kept for reporting. Absolute
    /// because the roots are resolved against
    /// [`repo_root`], which is derived from `CARGO_MANIFEST_DIR`.
    pub path: PathBuf,
    /// Language resolved from the file extension.
    pub lang: LANG,
    /// File contents.
    pub source: Vec<u8>,
}

/// A bounded, deterministic selection of corpus files.
pub struct CorpusSlice {
    /// Selected files, grouped by language and sorted by path within
    /// each language.
    pub files: Vec<CorpusFile>,
    /// Roots that were present on disk. Empty when the submodules are
    /// not checked out.
    pub roots: Vec<PathBuf>,
    /// Languages whose quota was cut short by
    /// [`Limits::max_total_bytes`], reported by
    /// [`CorpusSlice::summary`] so the ceiling never silently shrinks
    /// the slice behind a reader's back.
    pub truncated: Vec<&'static str>,
}

impl CorpusSlice {
    /// Loads the slice from [`CORPUS_ROOTS`] under `repo_root`.
    ///
    /// Missing roots are skipped rather than reported as an error: the
    /// corpus lives in git submodules that a fresh clone does not
    /// populate, and the synthetic benches are still worth running
    /// without them. Callers check [`CorpusSlice::is_empty`] and say so.
    #[must_use]
    pub fn load(repo_root: &Path) -> Self {
        Self::from_roots(
            &CORPUS_ROOTS
                .iter()
                .map(|r| repo_root.join(r))
                .collect::<Vec<_>>(),
        )
    }

    /// Loads the slice from an explicit set of roots, at the default
    /// [`Limits`].
    #[must_use]
    pub fn from_roots(roots: &[PathBuf]) -> Self {
        Self::from_roots_with(roots, Limits::default())
    }

    /// Loads the slice from an explicit set of roots and limits.
    #[must_use]
    pub fn from_roots_with(roots: &[PathBuf], limits: Limits) -> Self {
        let present: Vec<PathBuf> = roots.iter().filter(|r| r.is_dir()).cloned().collect();

        let mut by_lang: Candidates = BTreeMap::new();
        // Shared across roots: two roots can alias the same tree
        // through a symlink just as one root can alias itself.
        let mut visited = HashSet::new();
        for root in &present {
            collect_candidates(root, &mut visited, &mut by_lang);
        }

        let mut files = Vec::new();
        let mut total = 0_usize;
        let mut truncated = Vec::new();
        for (_, (lang, mut paths)) in by_lang {
            paths.sort();
            let mut taken = 0;
            for path in paths {
                if taken == limits.files_per_language {
                    break;
                }
                if total >= limits.max_total_bytes {
                    // Languages are visited in name order, so the byte
                    // ceiling truncates the tail of the alphabet rather
                    // than thinning every language evenly. Recorded so
                    // `summary` can say a language was dropped instead
                    // of silently omitting it from the breakdown.
                    truncated.push(lang.name());
                    break;
                }
                let Ok(source) = fs::read(&path) else {
                    continue;
                };
                if source.len() < limits.min_file_bytes || source.len() > limits.max_file_bytes {
                    continue;
                }
                total += source.len();
                taken += 1;
                files.push(CorpusFile { path, lang, source });
            }
        }

        Self {
            files,
            roots: present,
            truncated,
        }
    }

    /// Whether no file was selected — the usual cause is uninitialised
    /// submodules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Total bytes across the selected files.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.files.iter().map(|f| f.source.len()).sum()
    }

    /// A human-readable account of what the slice actually contains.
    ///
    /// Printed by every bench that measures over the corpus, so a
    /// number quoted from a run can be checked against the input that
    /// produced it rather than against an assumption about it.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return format!(
                "corpus slice: empty — none of {CORPUS_ROOTS:?} is a populated \
                 directory.\nRun `git submodule update --init --recursive` to \
                 fetch them; the synthetic shapes below do not need them.",
            );
        }

        let mut per_lang: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
        for file in &self.files {
            let entry = per_lang.entry(file.lang.name()).or_default();
            entry.0 += 1;
            entry.1 += file.source.len();
        }

        let header = format!(
            "corpus slice: {} files, {} KiB, from {} root(s)",
            self.files.len(),
            self.total_bytes() / 1_024,
            self.roots.len(),
        );
        let capped = self
            .truncated
            .iter()
            .map(|lang| format!("  NOTE  {lang} was cut short by the slice byte ceiling"));
        let roots = self
            .roots
            .iter()
            .map(|root| format!("  root  {}", root.display()));
        let langs = per_lang.into_iter().map(|(lang, (count, bytes))| {
            format!(
                "  {lang:<12} {count:>3} files  {kib:>6} KiB",
                kib = bytes / 1_024
            )
        });
        std::iter::once(header)
            .chain(roots)
            .chain(capped)
            .chain(langs)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Repository root, derived from this crate's manifest directory.
///
/// The bench targets need it to reach `tests/repositories/`, which is
/// outside the crate.
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Candidate paths bucketed by language.
///
/// Keyed by `LANG::name()` rather than by `LANG`: the enum is `Hash`
/// but not `Ord`, and the key ordering is what makes selection
/// deterministic across runs.
type Candidates = BTreeMap<&'static str, (LANG, Vec<PathBuf>)>;

/// Walks `dir` in sorted order, bucketing recognised source files by
/// language.
///
/// `visited` holds the canonical path of every directory already
/// walked, and is shared across roots. `Path::is_dir` follows
/// symlinks, so without it a symlinked directory is walked a second
/// time under its link path and every file beneath it enters the
/// candidate list twice. That is not hypothetical: the corpus ships
/// `DeepSpeech/tensorflow/native_client -> ../native_client`, which
/// duplicated the whole Java bucket — [`CorpusSlice::summary`]
/// reported sixteen files where there were eight, each measured
/// twice. The same set also bounds the recursion: a symlink cycle
/// would otherwise descend until the stack overflows, and a stack
/// overflow aborts rather than unwinds.
fn collect_candidates(dir: &Path, visited: &mut HashSet<PathBuf>, by_lang: &mut Candidates) {
    // A directory that cannot be canonicalised is a broken symlink or
    // one we cannot stat; either way there is nothing to walk.
    let Ok(canonical) = fs::canonicalize(dir) else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    names.sort();
    for path in names {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with('.') || SKIPPED_DIRS.contains(&name) {
            continue;
        }
        if path.is_dir() {
            collect_candidates(&path, visited, by_lang);
        } else if let Some(lang) = get_language_for_file(&path) {
            by_lang
                .entry(lang.name())
                .or_insert_with(|| (lang, Vec::new()))
                .1
                .push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use big_code_analysis::LANG;

    use super::{CORPUS_ROOTS, CorpusSlice, Limits, repo_root};

    /// An absent corpus yields an empty slice and a summary that says
    /// so, rather than a panic or a silently-zero measurement.
    #[test]
    fn absent_roots_produce_an_empty_slice() {
        let slice = CorpusSlice::from_roots(&[PathBuf::from("/nonexistent/corpus/root")]);
        assert!(slice.is_empty());
        assert!(slice.roots.is_empty());
        assert_eq!(slice.total_bytes(), 0);
        assert!(
            slice.summary().contains("empty"),
            "an empty slice must say so: {}",
            slice.summary(),
        );
    }

    /// The crate's own `src/` is a stand-in corpus root: it is always
    /// present, so this exercises selection, the per-language quota,
    /// and the summary without depending on submodules.
    #[test]
    fn selection_respects_the_quota_and_reports_itself() {
        let slice = CorpusSlice::from_roots(&[repo_root().join("big-code-analysis-bench/src")]);
        assert!(
            !slice.is_empty(),
            "the crate's own sources must be selectable"
        );
        let limits = Limits::default();
        assert!(slice.files.len() <= limits.files_per_language);
        assert!(
            slice.truncated.is_empty(),
            "the crate's own src fits the ceiling"
        );
        for file in &slice.files {
            assert!(file.source.len() <= limits.max_file_bytes);
            assert!(file.source.len() >= limits.min_file_bytes);
        }
        let summary = slice.summary();
        assert!(
            summary.contains(LANG::Rust.name()),
            "summary must name the language: {summary}",
        );
        assert!(
            summary.contains(&format!("{} files", slice.files.len())),
            "summary must report the file count it selected: {summary}",
        );
    }

    /// Selection is deterministic: two loads of the same roots pick
    /// the same files in the same order. Without this the criterion
    /// numbers would not be comparable across runs.
    #[test]
    fn selection_is_deterministic() {
        let root = repo_root().join("big-code-analysis-bench/src");
        let first = CorpusSlice::from_roots(std::slice::from_ref(&root));
        let second = CorpusSlice::from_roots(std::slice::from_ref(&root));
        let paths = |slice: &CorpusSlice| -> Vec<PathBuf> {
            slice.files.iter().map(|f| f.path.clone()).collect()
        };
        assert_eq!(paths(&first), paths(&second));
    }

    /// A symlinked directory does not duplicate the files beneath it.
    ///
    /// `Path::is_dir` follows symlinks, so an aliased subtree is
    /// otherwise walked twice and every file in it enters the
    /// candidate list under two paths. The corpus ships exactly this
    /// (`DeepSpeech/tensorflow/native_client -> ../native_client`),
    /// and it duplicated the entire Java bucket: `summary` claimed
    /// sixteen files where there were eight, each of them benched
    /// twice.
    ///
    /// Unix-only, and the attribute sits on the function rather than
    /// inside it: creating a directory symlink needs elevated
    /// privileges on Windows, and per lesson #40 a `#[cfg]` *inside* a
    /// test body would make the test pass vacuously there instead of
    /// visibly not existing.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_is_walked_once() {
        let temp = tempfile::tempdir().expect("temp dir");
        let real = temp.path().join("real");
        std::fs::create_dir(&real).expect("mkdir");
        std::fs::write(real.join("only.rs"), vec![b'\n'; 1_024]).expect("write");
        std::os::unix::fs::symlink(&real, temp.path().join("alias")).expect("symlink");

        let slice = CorpusSlice::from_roots(&[temp.path().to_path_buf()]);
        assert_eq!(
            slice.files.len(),
            1,
            "the aliased file was selected {} times: {:?}",
            slice.files.len(),
            slice.files.iter().map(|f| &f.path).collect::<Vec<_>>(),
        );
    }

    /// A symlink cycle terminates instead of recursing until the stack
    /// overflows.
    ///
    /// A stack overflow raises `SIGABRT` rather than a catchable panic
    /// (lesson #81), so the failure mode here would be an aborted
    /// process, not a failing test.
    #[cfg(unix)]
    #[test]
    fn a_symlink_cycle_terminates() {
        let temp = tempfile::tempdir().expect("temp dir");
        let nested = temp.path().join("nested");
        std::fs::create_dir(&nested).expect("mkdir");
        std::fs::write(nested.join("leaf.rs"), vec![b'\n'; 1_024]).expect("write");
        std::os::unix::fs::symlink(temp.path(), nested.join("loop")).expect("symlink");

        let slice = CorpusSlice::from_roots(&[temp.path().to_path_buf()]);
        assert_eq!(slice.files.len(), 1);
    }

    /// Hitting the byte ceiling is recorded and reported, not silently
    /// applied.
    ///
    /// A slice that quietly shrank would look like a complete
    /// measurement in the summary, which is the exact failure the
    /// summary exists to prevent.
    #[test]
    fn hitting_the_byte_ceiling_is_reported() {
        let limits = Limits {
            max_total_bytes: 1,
            ..Limits::default()
        };
        let slice = CorpusSlice::from_roots_with(
            &[repo_root().join("big-code-analysis-bench/src")],
            limits,
        );
        // The ceiling is checked before each file rather than after,
        // so one file always gets in and the cut lands on the second.
        assert_eq!(slice.files.len(), 1, "a 1-byte ceiling admits one file");
        assert_eq!(slice.truncated, vec!["rust"]);
        assert!(
            slice.summary().contains("byte ceiling"),
            "a truncated slice must say so: {}",
            slice.summary(),
        );
    }

    /// The declared roots are spelled relative to the repository root.
    /// A typo here would silently degrade every corpus bench to the
    /// empty slice.
    #[test]
    fn corpus_roots_are_repo_relative() {
        for root in CORPUS_ROOTS {
            assert!(
                root.starts_with("tests/repositories/"),
                "{root} must be a repo-relative submodule path",
            );
        }
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(repo_root().join("tests/repositories").is_dir());
    }
}
