//! Unit tests for `bca.toml` manifest parsing and merge logic.
//!
//! Discovery (which reads the process working directory) and the
//! end-to-end CLI precedence are exercised by the integration tests in
//! `tests/cli_ux/manifest.rs`; these cover the pure transforms in isolation.

use super::*;

/// Build a `Manifest` rooted at `/repo` from a raw payload, for tests
/// that exercise the merge/extract logic without touching the disk.
fn manifest(raw: RawManifest) -> Manifest {
    Manifest {
        dir: PathBuf::from("/repo"),
        path: PathBuf::from("/repo/bca.toml"),
        raw,
        // These transforms never read disk text, so default the job-count
        // key to the canonical spelling; the alias-attribution path is
        // covered by the integration tests in `tests/cli_ux/manifest.rs`.
        jobs_key: Some("jobs"),
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
        Some(&crate::threshold_soft::SoftLimit::Absolute(13.0))
    );
}

/// `[thresholds.lang.<slug>]` reaches [`Manifest::thresholds`] through
/// the same untyped `[thresholds]` map the `soft` sub-table uses (#1141),
/// so it needs no `RawManifest` field of its own — and, because
/// `KNOWN_SUB_TABLES` deliberately does not walk `[thresholds]`, no
/// allowlist entry either. Pin both halves: the nesting is split out as
/// a per-language layer, *and* it draws no "unrecognized key" warning.
#[test]
fn thresholds_lang_subtable_is_split_out_and_draws_no_warning() {
    let text = "[thresholds]\n\
                cognitive = 15\n\
                [thresholds.lang.c]\n\
                cognitive = 25\n";
    let raw: RawManifest = toml::from_str(text).expect("parse");
    let parsed = manifest(raw).thresholds();

    assert_eq!(parsed.hard.get("cognitive"), Some(&15.0));
    assert!(
        !parsed.hard.contains_key("lang"),
        "the lang sub-table must not be treated as a scalar limit"
    );
    assert_eq!(parsed.hard.len(), 1);
    assert_eq!(parsed.lang["c"].get("cognitive"), Some(&25.0));

    assert!(
        unknown_top_level_keys(text).is_empty(),
        "`thresholds` is already an allowlisted top-level key"
    );
    assert!(
        unknown_sub_table_keys(text).is_empty(),
        "`[thresholds]` keys are validated by split_thresholds_table, not the allowlist"
    );
}

#[test]
fn num_jobs_accepts_string_and_integer() {
    let auto = manifest(RawManifest {
        jobs: Some(toml::Value::String("auto".to_owned())),
        ..Default::default()
    });
    assert_eq!(auto.num_jobs(), Some(NumJobs::Auto));

    let four = manifest(RawManifest {
        jobs: Some(toml::Value::Integer(4)),
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
    // The manifest's `exclude_from` resolves against the manifest dir
    // and lands in the manifest-anchored set, not in the CLI's
    // `exclude_from` — the two anchor differently (#1164).
    let manifest_excludes = g.manifest_excludes.expect("manifest merged");
    assert_eq!(
        manifest_excludes.globs_from,
        Some(PathBuf::from("/repo/.bcaignore"))
    );
    assert_eq!(manifest_excludes.dir, PathBuf::from("/repo"));
    assert_eq!(g.exclude_from, None);
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

    // The union is by *effect*, not by list (#1164): each half keeps
    // its own list so each can keep its own anchor. The CLI's is
    // untouched, the manifest's is carried beside it with the manifest
    // directory, and the resolved view is CLI patterns first.
    let manifest_excludes = g.manifest_excludes.expect("manifest merged");
    assert_eq!(g.exclude, vec!["build".to_owned()]);
    assert_eq!(manifest_excludes.globs, vec!["vendor".to_owned()]);
    assert_eq!(manifest_excludes.dir, PathBuf::from("/repo"));
    assert_eq!(
        manifest_excludes.union_globs(&g.exclude),
        vec!["build".to_owned(), "vendor".to_owned()]
    );
}

/// The inline `exclude` list unions with the CLI's, but the
/// `exclude_from` *file* is replaced by a CLI `--exclude-from` — the
/// asymmetry predates #1164 and must survive the split that moved the
/// manifest's value out of `g.exclude_from` into its own field, where
/// it would otherwise have started unioning silently.
#[test]
fn merge_globals_exclude_from_file_is_replaced_by_the_cli_not_unioned() {
    let m = manifest(RawManifest {
        exclude: Some(vec!["vendor".to_owned()]),
        exclude_from: Some(PathBuf::from(".bcaignore")),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        exclude_from: Some(PathBuf::from("cli.ignore")),
        ..Default::default()
    };
    m.merge_globals(&mut g, false);

    let manifest_excludes = g.manifest_excludes.expect("manifest merged");
    assert_eq!(g.exclude_from, Some(PathBuf::from("cli.ignore")));
    assert_eq!(
        manifest_excludes.globs_from, None,
        "a CLI --exclude-from replaces the manifest's file"
    );
    // The inline list is untouched by that replacement.
    assert_eq!(manifest_excludes.globs, vec!["vendor".to_owned()]);
}

/// Duplicate patterns across CLI and manifest collapse to one in the
/// *resolved* view, order preserved (CLI first). Matching keeps the two
/// sets apart, where a duplicate is harmless — both sets say "exclude"
/// — so the dedup is a reporting rule now (#1164).
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

    let manifest_excludes = g.manifest_excludes.expect("manifest merged");
    assert_eq!(
        manifest_excludes.union_globs(&g.exclude),
        vec!["a".to_owned(), "b".to_owned()]
    );
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
    // counting `?` (#409) when the CLI left the flag unset (`None`).
    let m = manifest(RawManifest {
        cyclomatic_count_try: Some(false),
        ..Default::default()
    });
    let mut g = GlobalOpts::default();
    assert_eq!(g.count_cyclomatic_try, None);
    m.merge_globals(&mut g, false);
    assert_eq!(g.count_cyclomatic_try, Some(false));
}

#[test]
fn merge_globals_cyclomatic_count_try_default_keeps_counting() {
    // Absent key leaves the resolved value `None`, so the downstream
    // `unwrap_or(true)` default counts `?` and published metric values
    // are preserved (#409). An explicit `true` key carries through too.
    let mut g = GlobalOpts::default();
    manifest(RawManifest::default()).merge_globals(&mut g, false);
    assert_eq!(g.count_cyclomatic_try, None);

    let mut g_true = GlobalOpts::default();
    manifest(RawManifest {
        cyclomatic_count_try: Some(true),
        ..Default::default()
    })
    .merge_globals(&mut g_true, false);
    assert_eq!(g_true.count_cyclomatic_try, Some(true));
}

#[test]
fn merge_globals_cyclomatic_count_try_cli_value_wins_both_directions() {
    // Full override (#666): an explicit CLI value wins over the manifest
    // in EITHER direction. CLI `false` over manifest `true`, and — the
    // case the old OR-merge could not express — CLI `true` over manifest
    // `false`.
    let m_true = manifest(RawManifest {
        cyclomatic_count_try: Some(true),
        ..Default::default()
    });
    let mut g_off = GlobalOpts {
        count_cyclomatic_try: Some(false),
        ..Default::default()
    };
    m_true.merge_globals(&mut g_off, false);
    assert_eq!(g_off.count_cyclomatic_try, Some(false));

    let m_false = manifest(RawManifest {
        cyclomatic_count_try: Some(false),
        ..Default::default()
    });
    let mut g_on = GlobalOpts {
        count_cyclomatic_try: Some(true),
        ..Default::default()
    };
    m_false.merge_globals(&mut g_on, false);
    assert_eq!(g_on.count_cyclomatic_try, Some(true));
}

#[test]
fn merge_globals_exclude_tests_opts_in() {
    // Manifest `exclude_tests = true` turns on test-subtree pruning (#717)
    // when the CLI left the flag unset (`g.exclude_tests = false`).
    let m = manifest(RawManifest {
        exclude_tests: Some(true),
        ..Default::default()
    });
    let mut g = GlobalOpts::default();
    assert!(!g.exclude_tests);
    m.merge_globals(&mut g, false);
    assert!(g.exclude_tests);

    // Absent key leaves pruning off — published defaults preserved.
    let mut g_absent = GlobalOpts::default();
    manifest(RawManifest::default()).merge_globals(&mut g_absent, false);
    assert!(!g_absent.exclude_tests);

    // Explicit `exclude_tests = false` with the CLI flag unset stays off:
    // the key can only turn pruning on, and `Some(false)` must not be
    // mistaken for "set" — this pins the `unwrap_or(false)` value-merge
    // against an `is_some()`-style regression that the absent (`None`)
    // case alone would not catch.
    let mut g_explicit_off = GlobalOpts::default();
    manifest(RawManifest {
        exclude_tests: Some(false),
        ..Default::default()
    })
    .merge_globals(&mut g_explicit_off, false);
    assert!(!g_explicit_off.exclude_tests);
}

#[test]
fn merge_globals_exclude_tests_cli_flag_wins() {
    // `--exclude-tests` is presence-only (#717): once the flag set
    // `g.exclude_tests = true`, it stays on regardless of the manifest —
    // including an explicit `exclude_tests = false`, which the one-way
    // OR-merge cannot use to turn pruning back off.
    let m_off = manifest(RawManifest {
        exclude_tests: Some(false),
        ..Default::default()
    });
    let mut g = GlobalOpts {
        exclude_tests: true,
        ..Default::default()
    };
    m_off.merge_globals(&mut g, false);
    assert!(g.exclude_tests);
}

#[test]
fn merge_globals_respects_explicit_cli_num_jobs() {
    let m = manifest(RawManifest {
        jobs: Some(toml::Value::Integer(8)),
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

/// `exclude_tests` (#717) must be on the allowlist; otherwise the typed
/// parse honors it while `warn_unknown_keys` prints a misleading
/// "ignoring unrecognized key" warning (the #409 dual-update trap).
#[test]
fn known_keys_covers_exclude_tests() {
    let text = "exclude_tests = true\n";

    // Typed view honors it...
    let raw: RawManifest = toml::from_str(text).expect("typed parse");
    assert_eq!(raw.exclude_tests, Some(true));

    // ...and the allowlist agrees, so no spurious warning fires.
    assert!(
        unknown_top_level_keys(text).is_empty(),
        "exclude_tests is consumed but flagged as unknown"
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

/// Every key each typed sub-table consumes must appear in its per-table
/// allowlist (`CHECK_KEYS` / `REPORT_KEYS` / `VCS_KEYS`); otherwise the
/// typed parse honors it while `unknown_sub_table_keys` flags it as
/// unrecognized — the #409 dual-update trap, one nesting level down
/// (#843). Each `text` exercises one consumed field per sub-table.
#[test]
fn sub_table_allowlists_cover_consumed_keys() {
    // Every field RawCheck / RawReport / RawVcs deserializes, by table.
    for text in [
        "[check]\nexclude = [\"x\"]\n",
        "[check]\nexclude_from = \"i\"\n",
        "[check]\nexit_codes = \"tiered\"\n",
        "[check]\nbaseline = \"b\"\n",
        "[check]\nbaseline_line_tolerance = 2\n",
        "[check]\nbaseline_fuzzy_match = true\n",
        "[check]\nheadroom = 0.9\n",
        "[report]\nno_suppress = true\n",
        "[vcs]\nfile_types = \"all\"\n",
    ] {
        // The typed parse must accept it (proves the field is consumed)...
        toml::from_str::<RawManifest>(text).expect("typed parse");
        // ...and the allowlist must agree, so no spurious warning fires.
        assert!(
            unknown_sub_table_keys(text).is_empty(),
            "consumed sub-table key flagged as unknown: {text:?}"
        );
    }
}

/// A typo in a `[check]` / `[report]` / `[vcs]` sub-table key is surfaced
/// by `unknown_sub_table_keys` as `[<table>].<key>`, rather than silently
/// dropped by serde (#843). The reported name is fully qualified so the
/// warning points the user at the exact line.
#[test]
fn sub_table_typos_are_flagged_with_qualified_name() {
    assert_eq!(
        unknown_sub_table_keys("[check]\nbaseilne = \"x\"\n"),
        vec!["[check].baseilne".to_owned()]
    );
    assert_eq!(
        unknown_sub_table_keys("[report]\nno_supress = true\n"),
        vec!["[report].no_supress".to_owned()]
    );
    assert_eq!(
        unknown_sub_table_keys("[vcs]\nfile_type = \"all\"\n"),
        vec!["[vcs].file_type".to_owned()]
    );
    // `[thresholds]` is validated separately (`split_thresholds_table`),
    // so its keys are deliberately not walked here.
    assert!(unknown_sub_table_keys("[thresholds]\nbogus = 1\n").is_empty());
}

/// `[report] no_suppress = true` enables the audit view when the CLI
/// left `--no-suppress` unset (`None`); an explicit CLI value overrides
/// the manifest in EITHER direction (#683 full override).
#[test]
fn merge_report_enables_no_suppress_from_manifest() {
    let m = manifest(toml::from_str("[report]\nno_suppress = true\n").expect("parse"));
    let mut args = report_args(None);
    m.merge_report(&mut args);
    assert_eq!(
        args.no_suppress,
        Some(true),
        "manifest must enable the audit view"
    );

    // Absent manifest key leaves the resolved value `None`, so the
    // downstream default honors markers.
    let m_default = manifest(RawManifest::default());
    let mut honor = report_args(None);
    m_default.merge_report(&mut honor);
    assert_eq!(honor.no_suppress, None, "default honors markers");

    // CLI `false` forces the marker-honoring default even when the
    // manifest enabled the audit view — the case the old OR-merge could
    // not express.
    let mut cli_off = report_args(Some(false));
    m.merge_report(&mut cli_off);
    assert_eq!(
        cli_off.no_suppress,
        Some(false),
        "CLI false overrides manifest true"
    );
}

/// Build a `ReportArgs` for merge tests with the given `no_suppress`
/// flag; other fields take their non-interesting defaults.
fn report_args(no_suppress: Option<bool>) -> ReportArgs {
    ReportArgs {
        selection: crate::WalkSelectionArgs::default(),
        tuning: crate::WalkTuningArgs::default(),
        preproc: crate::PreprocConsumeArgs::default(),
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

/// Build a `CheckArgs` with everything unset, so a merge test observes
/// only what the manifest fills in. Routing through clap keeps the
/// struct in lockstep with the real CLI surface.
fn empty_check_args() -> CheckArgs {
    use clap::Parser;
    match crate::Cli::try_parse_from(["bca", "check"])
        .expect("check parses")
        .command
    {
        crate::Command::Check(args) => *args,
        other => panic!("expected Command::Check, got {other:?}"),
    }
}

/// The canonical `[check]` spelling of the four moved keys (#599) is
/// honoured by `merge_check`.
#[test]
fn merge_check_honours_check_table_baseline_keys() {
    let m = manifest(
        toml::from_str(
            "[check]\n\
             baseline = \"bl.toml\"\n\
             baseline_line_tolerance = 3\n\
             baseline_fuzzy_match = true\n\
             headroom = 0.9\n",
        )
        .expect("parse"),
    );
    let mut args = empty_check_args();
    // A bare soft tier so the manifest `[check] headroom` ratio folds in
    // (issue #688): it now drives the tier's ratio, not `args.headroom`.
    args.tier = crate::TierSpec::Soft(None);
    m.merge_check(&mut args);
    assert_eq!(args.baseline, Some(PathBuf::from("/repo/bl.toml")));
    assert_eq!(args.baseline_line_tolerance, Some(3));
    assert_eq!(args.baseline_fuzzy_match, Some(true));
    assert_eq!(args.tier, crate::TierSpec::Soft(Some(0.9)));
}

/// The deprecated top-level spelling (#599) is still honoured for one
/// release cycle, so existing manifests keep working.
#[test]
fn merge_check_honours_legacy_top_level_baseline_keys() {
    let m = manifest(
        toml::from_str(
            "baseline = \"bl.toml\"\n\
             baseline_line_tolerance = 3\n\
             baseline_fuzzy_match = true\n\
             headroom = 0.9\n",
        )
        .expect("parse"),
    );
    let mut args = empty_check_args();
    args.tier = crate::TierSpec::Soft(None);
    m.merge_check(&mut args);
    assert_eq!(args.baseline, Some(PathBuf::from("/repo/bl.toml")));
    assert_eq!(args.baseline_line_tolerance, Some(3));
    assert_eq!(args.baseline_fuzzy_match, Some(true));
    assert_eq!(args.tier, crate::TierSpec::Soft(Some(0.9)));
}

/// When a key is set in BOTH the deprecated top level and the
/// canonical `[check]` table, `[check]` wins (#599).
#[test]
fn merge_check_prefers_check_table_over_legacy_top_level() {
    let m = manifest(
        toml::from_str(
            "baseline = \"old.toml\"\n\
             baseline_line_tolerance = 1\n\
             headroom = 0.5\n\
             [check]\n\
             baseline = \"new.toml\"\n\
             baseline_line_tolerance = 9\n\
             headroom = 0.95\n",
        )
        .expect("parse"),
    );
    let mut args = empty_check_args();
    args.tier = crate::TierSpec::Soft(None);
    m.merge_check(&mut args);
    assert_eq!(args.baseline, Some(PathBuf::from("/repo/new.toml")));
    assert_eq!(args.baseline_line_tolerance, Some(9));
    assert_eq!(args.tier, crate::TierSpec::Soft(Some(0.95)));
}

/// `merge_exemptions` reads the baseline from `[check]` too (#599), so
/// the audit reflects exactly what `bca check` would skip.
#[test]
fn merge_exemptions_honours_check_table_baseline() {
    use clap::Parser;
    let m = manifest(toml::from_str("[check]\nbaseline = \"bl.toml\"\n").expect("parse"));
    let mut args = match crate::Cli::try_parse_from(["bca", "exemptions"])
        .expect("exemptions parses")
        .command
    {
        crate::Command::Exemptions(args) => args,
        other => panic!("expected Command::Exemptions, got {other:?}"),
    };
    assert!(args.baseline.is_none(), "no CLI flag → unset");
    m.merge_exemptions(&mut args);
    assert_eq!(args.baseline, Some(PathBuf::from("/repo/bl.toml")));
}

/// The deprecated top-level keys must stay on `KNOWN_KEYS` so they draw
/// only the move-deprecation warning, never the misleading "ignoring
/// unrecognized key" notice (#599; same regression class as #409).
#[test]
fn legacy_top_level_baseline_keys_are_known() {
    for text in [
        "baseline = \"x\"\n",
        "baseline_line_tolerance = 3\n",
        "baseline_fuzzy_match = true\n",
        "headroom = 0.9\n",
    ] {
        assert!(
            unknown_top_level_keys(text).is_empty(),
            "legacy key flagged as unknown: {text:?}"
        );
    }
}

#[test]
fn vcs_is_a_known_manifest_key() {
    // A `[vcs]` table must not draw the "ignoring unrecognized key"
    // warning (the #409 regression class): every consumed key is listed
    // in KNOWN_KEYS.
    assert!(unknown_top_level_keys("[vcs]\nfile_types = \"all\"\n").is_empty());
}
