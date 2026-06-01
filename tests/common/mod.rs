#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::float_cmp,
    clippy::needless_pass_by_value,
    clippy::needless_raw_string_hashes,
    clippy::similar_names,
    clippy::too_many_lines
)]

use std::path::Path;
use std::path::PathBuf;

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::{DirEntry, WalkDir};

use big_code_analysis::LANG;
use big_code_analysis::*;

#[allow(dead_code)]
pub mod fixtures;

#[allow(dead_code)]
pub mod validators;

#[allow(dead_code)]
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/", "repositories");
const SNAPSHOT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/",
    "repositories/big-code-analysis-output/snapshots"
);

#[derive(Debug)]
struct Config {
    language: Option<LANG>,
    source_root: PathBuf,
}

fn act_on_file(path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    // Open file
    let Some(source) = read_file_with_eol(&path)? else {
        return Ok(());
    };

    // Guess programming language
    let language = if let Some(language) = cfg.language {
        language
    } else if let Some(language) = guess_language(&source, &path).0 {
        language
    } else {
        return Ok(());
    };

    // Get FuncSpace struct.
    //
    // Snapshot fixtures key on the file path as the top-level
    // identifier, so use `Source::name` to thread the path string
    // through `analyze`. This matches the behaviour the deprecated
    // `get_function_spaces` shim had (lossy-stringified path) for
    // the valid-UTF-8 paths the integration corpora carry.
    let name = Some(path.to_string_lossy().into_owned());
    let funcspace_struct = analyze(
        Source::new(language, &source)
            .with_name(name)
            .with_preproc_path(Some(&path)),
        MetricsOptions::default(),
    )
    .expect("analyze returned Err for fixture; the parser may have rejected the source");

    insta::with_settings!({snapshot_path => Path::new(SNAPSHOT_PATH)
                .join(path.strip_prefix(&cfg.source_root).unwrap())
                .parent()
                .unwrap(),
                prepend_module_to_snapshot => false,
                sort_maps => true,
    }, {
        insta::assert_yaml_snapshot!(
            path.file_name().unwrap().to_string_lossy().as_ref(),
            funcspace_struct,
            {
                // Round floating point values to three decimal places since the can differ from
                // system to system.
                ".spaces[].**.metrics.*.*" => insta::rounded_redaction(3),
                ".metrics.*.*" => insta::rounded_redaction(3),
                // Redact away the name since paths are different on different systems.
                ".name" => "[filepath]",
            }
        );

    });

    Ok(())
}

/// Produces metrics runtime and compares them with previously generated json files
#[allow(dead_code)]
pub fn compare_rca_output_with_files(repo_name: &str, include: &[&str], exclude: &[&str]) {
    compare_rca_output_with_files_under(Path::new(REPO), repo_name, include, exclude);
}

/// Same as [`compare_rca_output_with_files`] but with an explicit source root.
///
/// `source_root` is the directory whose layout mirrors the snapshot directory:
/// each input file's path under `source_root` becomes its snapshot path under
/// `SNAPSHOT_PATH`. Use this when the corpus lives nested under the
/// `big-code-analysis-output` submodule (as for the synthetic PHP corpus) so
/// snapshots land at `snapshots/<repo_name>/...` rather than picking up the
/// submodule directory as an extra path component.
#[allow(dead_code)]
pub fn compare_rca_output_with_files_under(
    source_root: &Path,
    repo_name: &str,
    include: &[&str],
    exclude: &[&str],
) {
    let num_jobs = 4;

    let cfg = Config {
        language: None,
        source_root: source_root.to_path_buf(),
    };

    let mut gsbi = GlobSetBuilder::new();
    for file in include {
        gsbi.add(Glob::new(file).unwrap());
    }

    let mut gsbe = GlobSetBuilder::new();
    for file in exclude {
        gsbe.add(Glob::new(file).unwrap());
    }

    // The library runner is a terminal file-set processor (#495): it no
    // longer walks directories or applies globsets. Resolve the corpus
    // root into a filtered file list here — skipping hidden entries and
    // applying the include/exclude globsets against the emitted path —
    // then hand the runner the resolved list.
    let include = gsbi.build().unwrap();
    let exclude = gsbe.build().unwrap();
    let paths = resolve_corpus_files(&source_root.join(repo_name), &include, &exclude);

    let files_data = FilesData { paths };

    if let Err(e) = ConcurrentRunner::new(num_jobs, act_on_file).run(cfg, files_data) {
        // Use panic! rather than process::exit so the failure surfaces
        // through cargo test's per-test reporting and lets the rest of
        // the binary's tests produce their own diagnostics.
        panic!("ConcurrentRunner failed: {e:?}");
    }
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

/// Walk `root` recursively, returning the regular files that pass the
/// include/exclude globsets (matched against the emitted path, as the
/// pre-#495 library walk did) and aren't under a hidden directory.
#[allow(dead_code)]
fn resolve_corpus_files(root: &Path, include: &GlobSet, exclude: &GlobSet) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.is_file()
                && (include.is_empty() || include.is_match(path))
                && (exclude.is_empty() || !exclude.is_match(path))
        })
        .collect()
}
