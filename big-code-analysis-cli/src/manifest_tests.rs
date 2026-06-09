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

/// Negative filter key `exclude` UNIONs CLI and manifest patterns (#539):
/// a CLI `--exclude` must not silently drop a directory the manifest
/// deliberately skipped. This would FAIL under the pre-#539 replace
/// behaviour, where a non-empty CLI list discarded the manifest's.
#[test]
fn merge_globals_unions_exclude_with_manifest() {
    let m = manifest(RawManifest {
        exclude: Some(vec!["vendor".to_owned()]),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        exclude: vec!["build".to_owned()],
        ..Default::default()
    };
    m.merge_globals(&mut g, false);

    // CLI patterns first, then manifest patterns appended.
    assert_eq!(g.exclude, vec!["build".to_owned(), "vendor".to_owned()]);
}

/// Duplicate patterns across CLI and manifest collapse to one, order
/// preserved (CLI first).
#[test]
fn merge_globals_exclude_union_dedups() {
    let m = manifest(RawManifest {
        exclude: Some(vec!["a".to_owned(), "b".to_owned()]),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        exclude: vec!["a".to_owned()],
        ..Default::default()
    };
    m.merge_globals(&mut g, false);

    assert_eq!(g.exclude, vec!["a".to_owned(), "b".to_owned()]);
}

/// Positive scope key `include` is REPLACED by any CLI value (pinned so a
/// future "make every list union" change is caught): manifest `include`
/// is dropped when the CLI supplied its own.
#[test]
fn merge_globals_include_replaces_not_unions() {
    let m = manifest(RawManifest {
        include: Some(vec!["*.py".to_owned()]),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        include: vec!["*.rs".to_owned()],
        ..Default::default()
    };
    m.merge_globals(&mut g, false);

    assert_eq!(g.include, vec!["*.rs".to_owned()]);
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

/// Every key `RawManifest` consumes must appear in `KNOWN_KEYS`;
/// otherwise the typed parse honors it while `warn_unknown_keys`
/// prints a misleading "ignoring unrecognized key" warning. This
/// regression guards the #409 `cyclomatic_count_try` case, where the
/// field was added to the typed view but omitted from the allowlist.
#[test]
fn known_keys_covers_cyclomatic_count_try() {
    let text = "cyclomatic_count_try = false\n";

    // Typed view honors it...
    let raw: RawManifest = toml::from_str(text).expect("typed parse");
    assert_eq!(raw.cyclomatic_count_try, Some(false));

    // ...and the allowlist agrees, so no spurious warning fires.
    assert!(
        unknown_top_level_keys(text).is_empty(),
        "cyclomatic_count_try is consumed but flagged as unknown"
    );
}

/// The `[report]` table (#501) must be on the allowlist; otherwise the
/// typed parse honors it while `warn_unknown_keys` prints a misleading
/// "ignoring unrecognized key `report`" warning.
#[test]
fn known_keys_covers_report_table() {
    let text = "[report]\nno_suppress = true\n";
    let raw: RawManifest = toml::from_str(text).expect("typed parse");
    assert_eq!(raw.report.no_suppress, Some(true));
    assert!(
        unknown_top_level_keys(text).is_empty(),
        "[report] is consumed but flagged as unknown"
    );
}

/// `[report] no_suppress = true` enables the audit view when the CLI
/// did not pass `--no-suppress`; a bare CLI flag can still force it on,
/// but the manifest never forces it off (OR semantics, like
/// `baseline_fuzzy_match`).
#[test]
fn merge_report_enables_no_suppress_from_manifest() {
    let m = manifest(toml::from_str("[report]\nno_suppress = true\n").expect("parse"));
    let mut args = report_args(false);
    m.merge_report(&mut args);
    assert!(args.no_suppress, "manifest must enable the audit view");

    // Absent / false manifest key leaves an explicit CLI opt-in intact
    // and does not turn a default-honor run into audit.
    let m_default = manifest(RawManifest::default());
    let mut honor = report_args(false);
    m_default.merge_report(&mut honor);
    assert!(!honor.no_suppress, "default honors markers");
    let mut already_on = report_args(true);
    m_default.merge_report(&mut already_on);
    assert!(already_on.no_suppress, "CLI opt-in survives empty manifest");
}

/// Build a `ReportArgs` for merge tests with the given `no_suppress`
/// flag; other fields take their non-interesting defaults.
fn report_args(no_suppress: bool) -> ReportArgs {
    ReportArgs {
        format: Some(crate::formats::ReportFormat::Markdown),
        format_positional: None,
        output: None,
        top: 20,
        strip_prefix: String::new(),
        no_suppress,
        vcs: false,
    }
}

/// Parse `argv` into a `VcsArgs`, for the `[vcs]` merge tests. Routing
/// through clap keeps the args struct in lockstep with the real CLI
/// surface (no hand-rolled defaults to drift).
fn vcs_args(argv: &[&str]) -> VcsArgs {
    use clap::Parser;
    match crate::Cli::try_parse_from(argv)
        .expect("vcs parses")
        .command
    {
        crate::Command::Vcs(args) => *args,
        other => panic!("expected Command::Vcs, got {other:?}"),
    }
}

#[test]
fn merge_vcs_fills_file_types_when_cli_unset() {
    let m = manifest(toml::from_str("[vcs]\nfile_types = \"all\"\n").expect("parse"));
    let mut args = vcs_args(&["bca", "vcs"]);
    assert!(args.file_types.is_none(), "no CLI flag → unset");
    m.merge_vcs(&mut args);
    assert_eq!(
        args.file_types.as_deref(),
        Some("all"),
        "the manifest value fills an unset --file-types"
    );
}

#[test]
fn merge_vcs_cli_flag_replaces_manifest() {
    // `file_types` is a positive scope key: an explicit CLI value wins
    // outright (it replaces, never unions with, the manifest).
    let m = manifest(toml::from_str("[vcs]\nfile_types = \"all\"\n").expect("parse"));
    let mut args = vcs_args(&["bca", "vcs", "--file-types", "rs,py"]);
    m.merge_vcs(&mut args);
    assert_eq!(
        args.file_types.as_deref(),
        Some("rs,py"),
        "the CLI flag replaces the manifest value"
    );
}

#[test]
fn merge_vcs_empty_manifest_leaves_cli_unset() {
    let m = manifest(RawManifest::default());
    let mut args = vcs_args(&["bca", "vcs"]);
    m.merge_vcs(&mut args);
    assert!(
        args.file_types.is_none(),
        "no manifest and no CLI flag leaves the scope at its default"
    );
}

#[test]
fn vcs_is_a_known_manifest_key() {
    // A `[vcs]` table must not draw the "ignoring unrecognized key"
    // warning (the #409 regression class): every consumed key is listed
    // in KNOWN_KEYS.
    assert!(unknown_top_level_keys("[vcs]\nfile_types = \"all\"\n").is_empty());
}
