//! Unit tests for `bca.toml` manifest parsing and merge logic.
//!
//! Discovery (which reads the process working directory) and the
//! end-to-end CLI precedence are exercised by the integration tests in
//! `tests/manifest.rs`; these cover the pure transforms in isolation.

use super::*;

/// Build a `Manifest` rooted at `/repo` from a raw payload, for tests
/// that exercise the merge/extract logic without touching the disk.
fn manifest(raw: RawManifest) -> Manifest {
    Manifest {
        dir: PathBuf::from("/repo"),
        path: PathBuf::from("/repo/bca.toml"),
        raw,
    }
}

#[test]
fn thresholds_extracts_scalars_and_ignores_subtables() {
    let raw: RawManifest = toml::from_str(
        "[thresholds]\n\
         cyclomatic = 15\n\
         \"halstead.effort\" = 47500.0\n\
         [thresholds.soft]\n\
         cyclomatic = 13\n",
    )
    .expect("parse");
    let m = manifest(raw);
    let parsed = m.thresholds();

    // Both scalar forms (integer + float) land in the hard layer; the
    // `soft` sub-table (#375) is split into its own layer rather than
    // being mistaken for a scalar limit named "soft".
    assert_eq!(parsed.hard.get("cyclomatic"), Some(&15.0));
    assert_eq!(parsed.hard.get("halstead.effort"), Some(&47_500.0));
    assert!(
        !parsed.hard.contains_key("soft"),
        "the soft sub-table must not be treated as a scalar limit"
    );
    assert_eq!(parsed.hard.len(), 2);
    // The soft override is captured as an absolute limit.
    assert_eq!(
        parsed.soft.get("cyclomatic"),
        Some(&crate::thresholds::SoftLimit::Absolute(13.0))
    );
}

#[test]
fn num_jobs_accepts_string_and_integer() {
    let auto = manifest(RawManifest {
        num_jobs: Some(toml::Value::String("auto".to_owned())),
        ..Default::default()
    });
    assert_eq!(auto.num_jobs(), Some(NumJobs::Auto));

    let four = manifest(RawManifest {
        num_jobs: Some(toml::Value::Integer(4)),
        ..Default::default()
    });
    assert_eq!(
        four.num_jobs(),
        Some(NumJobs::Explicit(4.try_into().unwrap()))
    );

    let none = manifest(RawManifest::default());
    assert_eq!(none.num_jobs(), None);
}

#[test]
fn resolve_joins_relative_against_manifest_dir_and_keeps_absolute() {
    let m = manifest(RawManifest::default());
    assert_eq!(
        m.resolve(Path::new(".bcaignore")),
        PathBuf::from("/repo/.bcaignore")
    );
    assert_eq!(m.resolve(Path::new("/etc/x")), PathBuf::from("/etc/x"));
}

#[test]
fn merge_globals_fills_unset_and_resolves_relative_paths() {
    let m = manifest(RawManifest {
        paths: Some(vec![PathBuf::from("src"), PathBuf::from("/abs")]),
        exclude_from: Some(PathBuf::from(".bcaignore")),
        include: Some(vec!["*.rs".to_owned()]),
        ..Default::default()
    });
    let mut g = GlobalOpts::default();
    m.merge_globals(&mut g, false);

    // Relative manifest paths anchor to the manifest dir; absolute ones
    // pass through.
    assert_eq!(
        g.paths,
        vec![PathBuf::from("/repo/src"), PathBuf::from("/abs")]
    );
    assert_eq!(g.exclude_from, Some(PathBuf::from("/repo/.bcaignore")));
    assert_eq!(g.include, vec!["*.rs".to_owned()]);
}

#[test]
fn merge_globals_does_not_clobber_cli_values() {
    let m = manifest(RawManifest {
        paths: Some(vec![PathBuf::from("manifest_path")]),
        include: Some(vec!["from_manifest".to_owned()]),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        paths: vec![PathBuf::from("cli_path")],
        include: vec!["from_cli".to_owned()],
        ..Default::default()
    };
    m.merge_globals(&mut g, false);

    assert_eq!(g.paths, vec![PathBuf::from("cli_path")]);
    assert_eq!(g.include, vec!["from_cli".to_owned()]);
}

#[test]
fn merge_globals_cyclomatic_count_try_opts_out_when_false() {
    // Manifest `cyclomatic_count_try = false` opts the gate out of
    // counting `?` (#409) when the CLI flag is absent.
    let m = manifest(RawManifest {
        cyclomatic_count_try: Some(false),
        ..Default::default()
    });
    let mut g = GlobalOpts::default();
    assert!(!g.no_cyclomatic_try);
    m.merge_globals(&mut g, false);
    assert!(g.no_cyclomatic_try);
}

#[test]
fn merge_globals_cyclomatic_count_try_default_keeps_counting() {
    // Absent key (or `true`) leaves the default counting behaviour
    // intact, so published metric values are preserved (#409).
    for raw in [
        RawManifest::default(),
        RawManifest {
            cyclomatic_count_try: Some(true),
            ..Default::default()
        },
    ] {
        let mut g = GlobalOpts::default();
        manifest(raw).merge_globals(&mut g, false);
        assert!(!g.no_cyclomatic_try);
    }
}

#[test]
fn merge_globals_cyclomatic_count_try_cli_flag_wins() {
    // `--no-cyclomatic-try` ORs on top: a manifest `true` cannot turn
    // counting back on once the CLI flag has opted out (#409).
    let m = manifest(RawManifest {
        cyclomatic_count_try: Some(true),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        no_cyclomatic_try: true,
        ..Default::default()
    };
    m.merge_globals(&mut g, false);
    assert!(g.no_cyclomatic_try);
}

#[test]
fn merge_globals_respects_explicit_cli_num_jobs() {
    let m = manifest(RawManifest {
        num_jobs: Some(toml::Value::Integer(8)),
        ..Default::default()
    });

    // CLI set it → manifest is ignored, default Auto stays.
    let mut explicit = GlobalOpts::default();
    m.merge_globals(&mut explicit, true);
    assert_eq!(explicit.num_jobs, NumJobs::Auto);

    // CLI did not set it → manifest value applies.
    let mut from_manifest = GlobalOpts::default();
    m.merge_globals(&mut from_manifest, false);
    assert_eq!(
        from_manifest.num_jobs,
        NumJobs::Explicit(8.try_into().unwrap())
    );
}
