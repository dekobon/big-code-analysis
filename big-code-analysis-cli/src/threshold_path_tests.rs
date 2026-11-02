//! Unit tests for [`crate::threshold_path`].

use super::*;

/// Parse a `[thresholds]` table the way the manifest does, with every
/// path entry anchored at `/cfg`.
fn parse(toml_src: &str) -> Result<Vec<PathThreshold>, String> {
    let table: toml::Table = toml::from_str(toml_src).expect("parses as TOML");
    parse_path_entries(&table["path"], Path::new("/cfg"), "bca.toml")
}

fn limits(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs
        .iter()
        .map(|(name, limit)| ((*name).to_owned(), *limit))
        .collect()
}

#[test]
fn entries_parse_in_declaration_order_with_canonical_keys_and_the_origin() {
    let entries = parse(
        "[[path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 40\n\
         ploc = 900\n\
         [[path]]\n\
         glob = \"./vendor/*.c\"\n\
         \"halstead.effort\" = 150000.5\n",
    )
    .expect("two valid entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].glob, "src/**");
    assert_eq!(entries[0].origin, Path::new("/cfg"));
    // The bare `diff --metric` alias canonicalises at the parse boundary
    // (#1165), so a later `extend` overrides the dotted global limit.
    assert_eq!(
        entries[0].limits,
        limits(&[("cyclomatic", 40.0), ("loc.ploc", 900.0)])
    );
    assert_eq!(entries[1].glob, "./vendor/*.c", "echoed as written");
    assert_eq!(entries[1].limits, limits(&[("halstead.effort", 150_000.5)]));
}

#[test]
fn the_path_key_must_be_an_array_of_tables() {
    let err = parse("[path]\nglob = \"src/**\"\ncyclomatic = 1\n").expect_err("a table");
    assert!(err.contains("must be an array of tables"), "{err}");
    let err = parse("path = [1]\n").expect_err("a scalar element");
    assert!(
        err.contains("[[thresholds.path]] #1: expected a table"),
        "{err}"
    );
}

#[test]
fn an_entry_needs_a_non_empty_string_glob() {
    for (src, expected) in [
        ("[[path]]\ncyclomatic = 1\n", "missing the `glob` key"),
        (
            "[[path]]\nglob = 3\ncyclomatic = 1\n",
            "`glob` must be a string",
        ),
        (
            "[[path]]\nglob = \"\"\ncyclomatic = 1\n",
            "empty once its leading `./` is removed",
        ),
        (
            "[[path]]\nglob = \"./\"\ncyclomatic = 1\n",
            "empty once its leading `./` is removed",
        ),
        (
            "[[path]]\nglob = \"src/[\"\ncyclomatic = 1\n",
            "invalid glob",
        ),
    ] {
        let err = parse(src).expect_err(src);
        assert!(err.starts_with("[[thresholds.path]] #1"), "{src}: {err}");
        assert!(err.contains(expected), "{src}: {err}");
    }
}

#[test]
fn an_unknown_metric_names_the_entry_at_build_time_and_a_bad_value_at_parse_time() {
    // A spelling the alias layer does not know passes the parse — the
    // registry owns the did-you-mean list — and is rejected by
    // `ThresholdSet::build_tiered` with the entry's context prefixed by
    // the resolver. What the parse itself rejects, naming the entry, is
    // a non-numeric value and the ambiguous family head.
    let err = parse("[[path]]\nglob = \"src/**\"\ncyclomatic = \"many\"\n").expect_err("string");
    assert!(
        err.starts_with(
            "[[thresholds.path]] #1 (glob \"src/**\") \"cyclomatic\": expected a number"
        ),
        "{err}"
    );
    let err = parse("[[path]]\nglob = \"src/**\"\nhalstead = 1\n").expect_err("family head");
    assert!(
        err.starts_with("[[thresholds.path]] #1 (glob \"src/**\") ambiguous metric"),
        "{err}"
    );
    let err = parse(
        "[[path]]\nglob = \"a\"\nnargs = 1\n[[path]]\nglob = \"b\"\nploc = 1\n\"loc.ploc\" = 2\n",
    )
    .expect_err("two spellings of one metric");
    assert!(
        err.starts_with("[[thresholds.path]] #2 (glob \"b\")"),
        "{err}"
    );
}

#[test]
fn tokens_and_an_empty_entry_are_rejected() {
    let err = parse("[[path]]\nglob = \"src/**\"\ntokens = 5000\n").expect_err("tokens");
    assert!(
        err.contains("`tokens` cannot be overridden per path"),
        "{err}"
    );
    let err = parse("[[path]]\nglob = \"src/**\"\n").expect_err("no limits");
    assert!(err.contains("no `metric = <limit>` entries"), "{err}");
}

/// A resolver that records every table it was asked for and builds a
/// hard-tier set from it, so the tests can see exactly what was folded.
struct Recording(Mutex<Vec<(String, BTreeMap<String, f64>)>>);

impl HardResolver for Recording {
    fn resolve(&self, hard: BTreeMap<String, f64>, context: &str) -> ThresholdSet {
        if let Ok(mut seen) = self.0.lock() {
            seen.push((context.to_owned(), hard.clone()));
        }
        ThresholdSet::build_tiered(&hard, &hard).expect("test limits are valid")
    }
}

/// An entry as `bca.toml` at `origin` would declare it, numbered
/// `ordinal` within that file.
fn entry_at(ordinal: usize, glob: &str, origin: &str, pairs: &[(&str, f64)]) -> PathThreshold {
    PathThreshold {
        glob: glob.to_owned(),
        pattern: Glob::new(strip_dot_slash(glob)).expect("valid test glob"),
        origin: PathBuf::from(origin),
        ordinal,
        source: "bca.toml".to_owned(),
        limits: limits(pairs),
    }
}

/// The entries a manifest at `/cfg` declares, in order.
fn entries(specs: &[(&str, &[(&str, f64)])]) -> Vec<PathThreshold> {
    specs
        .iter()
        .enumerate()
        .map(|(index, (glob, pairs))| entry_at(index + 1, glob, "/cfg", pairs))
        .collect()
}

fn bases() -> BTreeMap<Option<&'static str>, BaseTable> {
    BTreeMap::from([
        (
            None,
            BaseTable {
                hard: limits(&[("cyclomatic", 15.0), ("cognitive", 15.0)]),
                context: None,
            },
        ),
        (
            Some("c"),
            BaseTable {
                hard: limits(&[("cyclomatic", 15.0), ("cognitive", 30.0)]),
                context: Some("[thresholds.lang.c]".to_owned()),
            },
        ),
    ])
}

fn resolved(set: &ThresholdSet) -> BTreeMap<&'static str, f64> {
    set.iter().collect()
}

#[test]
fn construction_resolves_each_entry_over_each_base_and_memoises_it() {
    let recording = Arc::new(Recording(Mutex::new(Vec::new())));
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0)]),
        ("src/gen/**", &[("cognitive", 50.0)]),
    ]);
    let resolver: Arc<Recording> = Arc::clone(&recording);
    let layer = PathLayer::new(&entries, bases(), resolver);
    let seen = recording.0.lock().expect("unpoisoned");
    let contexts: Vec<&str> = seen.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(
        contexts,
        [
            "[[thresholds.path]] #1 (glob \"src/**\") in bca.toml",
            "[[thresholds.path]] #2 (glob \"src/gen/**\") in bca.toml",
            "[thresholds.lang.c] + [[thresholds.path]] #1 (glob \"src/**\") in bca.toml",
            "[thresholds.lang.c] + [[thresholds.path]] #2 (glob \"src/gen/**\") in bca.toml",
        ],
        "every (base, entry) pair is validated eagerly, naming both"
    );
    assert_eq!(
        seen[2].1,
        limits(&[("cyclomatic", 40.0), ("cognitive", 30.0)]),
        "an entry folds onto the language's extended map, not the global one"
    );
    assert_eq!(layer.resolved_sets().len(), 4);
}

/// The eager validation at construction resolves each entry *alone*
/// over each base, and claims that is enough to surface every error a
/// match set could raise. That rests on every check in the resolver
/// being per metric: in any match set a metric's value comes from
/// exactly one entry, so a two-entry set can only fail where one of its
/// entries already fails alone. Pinned here by resolving a set whose
/// two entries are each valid alone and together, and whose *fold* is
/// the first thing the resolver sees of the combination.
#[test]
fn a_two_entry_match_set_resolves_through_the_same_resolver_as_its_entries() {
    let recording = Arc::new(Recording(Mutex::new(Vec::new())));
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0)]),
        ("src/gen/**", &[("cognitive", 50.0)]),
    ]);
    let resolver: Arc<Recording> = Arc::clone(&recording);
    let layer = PathLayer::new(&entries, bases(), resolver);
    let both = layer
        .for_file(
            Some("c"),
            Path::new("src/gen/a.c"),
            CwdForm(Path::new("src/gen/a.c")),
            Admission::Walked,
        )
        .expect("both entries match");
    assert_eq!(
        resolved(&both),
        BTreeMap::from([("cyclomatic", 40.0), ("cognitive", 50.0)])
    );
    let seen = recording.0.lock().expect("unpoisoned");
    let (context, hard) = seen.last().expect("the combination was resolved last");
    assert_eq!(
        context,
        "[thresholds.lang.c] + [[thresholds.path]] #1 (glob \"src/**\") in bca.toml + \
         [[thresholds.path]] #2 (glob \"src/gen/**\") in bca.toml"
    );
    assert_eq!(
        *hard,
        limits(&[("cyclomatic", 40.0), ("cognitive", 50.0)]),
        "each metric of the fold comes from exactly one entry"
    );
    assert_eq!(seen.len(), 5, "4 single-entry sets at construction + 1");
}

#[test]
fn the_last_matching_entry_wins_per_metric_and_unmatched_metrics_inherit() {
    let recording = Arc::new(Recording(Mutex::new(Vec::new())));
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0), ("cognitive", 20.0)]),
        ("src/gen/**", &[("cyclomatic", 60.0)]),
    ]);
    let layer = PathLayer::new(&entries, bases(), recording);
    let both = layer
        .for_file(
            None,
            Path::new("src/gen/a.rs"),
            CwdForm(Path::new("src/gen/a.rs")),
            Admission::Walked,
        )
        .expect("both entries match");
    assert_eq!(
        resolved(&both),
        BTreeMap::from([("cyclomatic", 60.0), ("cognitive", 20.0)]),
        "cyclomatic from the later entry, cognitive from the earlier, nothing from the base"
    );
    let first_only = layer
        .for_file(
            None,
            Path::new("src/a.rs"),
            CwdForm(Path::new("src/a.rs")),
            Admission::Walked,
        )
        .expect("the first entry matches");
    assert_eq!(
        resolved(&first_only),
        BTreeMap::from([("cyclomatic", 40.0), ("cognitive", 20.0)])
    );
    assert!(
        layer
            .for_file(
                None,
                Path::new("lib/a.rs"),
                CwdForm(Path::new("lib/a.rs")),
                Admission::Walked
            )
            .is_none(),
        "no match hands the caller back to the base set"
    );
    // The two-entry combination was resolved once, on first use, and the
    // single-entry ones at construction: 4 + 1.
    assert_eq!(layer.resolved_sets().len(), 5);
    layer.for_file(
        None,
        Path::new("src/gen/b.rs"),
        CwdForm(Path::new("src/gen/b.rs")),
        Admission::Walked,
    );
    assert_eq!(
        layer.resolved_sets().len(),
        5,
        "memoised per matched set, not per path"
    );
}

/// `resolved_sets` feeds `all_sets`, and `warn_limit_advisories` prints
/// the first advisory it meets per `(metric, limit)` — so the order
/// must be a function of the keys, never of a hash seed (#1091). Pinned
/// against a hand-written sequence: the memo is keyed `(slug, matched)`
/// and `None < Some(_)`, so the global base's sets come first, each
/// base's single entries in declaration order, and a combination
/// resolved later sorts by its index vector, not by when it arrived.
#[test]
fn resolved_sets_are_ordered_by_base_then_matched_entries() {
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0)]),
        ("src/gen/**", &[("cyclomatic", 60.0)]),
    ]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    layer.for_file(
        Some("c"),
        Path::new("src/gen/a.c"),
        CwdForm(Path::new("src/gen/a.c")),
        Admission::Walked,
    );
    layer.for_file(
        None,
        Path::new("src/gen/a.rs"),
        CwdForm(Path::new("src/gen/a.rs")),
        Admission::Walked,
    );
    let cyclomatic: Vec<(Option<f64>, Option<f64>)> = layer
        .resolved_sets()
        .iter()
        .map(|set| {
            let limits = resolved(set);
            (
                limits.get("cyclomatic").copied(),
                limits.get("cognitive").copied(),
            )
        })
        .collect();
    assert_eq!(
        cyclomatic,
        [
            (Some(40.0), Some(15.0)), // (None, [0])
            (Some(60.0), Some(15.0)), // (None, [1])
            (Some(60.0), Some(15.0)), // (None, [0, 1]) — resolved last, sorts here
            (Some(40.0), Some(30.0)), // (Some("c"), [0])
            (Some(60.0), Some(30.0)), // (Some("c"), [1])
            (Some(60.0), Some(30.0)), // (Some("c"), [0, 1])
        ]
    );
}

#[test]
fn a_language_base_composes_before_the_entry() {
    let entries = entries(&[("src/**", &[("cyclomatic", 40.0)])]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    let c = layer
        .for_file(
            Some("c"),
            Path::new("src/a.c"),
            CwdForm(Path::new("src/a.c")),
            Admission::Walked,
        )
        .expect("matches");
    assert_eq!(
        resolved(&c),
        BTreeMap::from([("cyclomatic", 40.0), ("cognitive", 30.0)]),
        "the path entry overrides cyclomatic; cognitive is C's own 30, not the project's 15"
    );
}

// unix: `/cfg` is a POSIX absolute path; on Windows it is relative, so
// `strip_prefix` against the origin never matches and every file takes
// the seed-form fallback — which is the *other* branch this test
// discriminates from. Fabricating a drive-letter origin would be a
// platform-shaped fixture (testing.md); the origin-relative branch is
// the same code on both targets, so one leg suffices.
#[cfg(unix)]
#[test]
fn a_file_inside_the_origin_is_matched_relative_to_it() {
    // The manifest lives at `/cfg` and the glob is written against it.
    // Run from anywhere, an absolute path under `/cfg` strips to its
    // manifest-relative form; one outside it falls back to the
    // seed-relative form exactly as a `[check] exclude` glob would.
    let entries = entries(&[("src/*.rs", &[("cyclomatic", 40.0)])]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    assert!(
        layer
            .for_file(
                None,
                Path::new("/cfg/src/a.rs"),
                CwdForm(Path::new("../a.rs")),
                Admission::Walked
            )
            .is_some(),
        "absolute path under the origin matches the origin-relative glob"
    );
    assert!(
        layer
            .for_file(
                None,
                Path::new("/elsewhere/src/a.rs"),
                CwdForm(Path::new("src/a.rs")),
                Admission::Walked
            )
            .is_some(),
        "a file outside the origin is matched by its seed-relative form"
    );
    assert!(
        layer
            .for_file(
                None,
                Path::new("/elsewhere/src/a.rs"),
                CwdForm(Path::new("/elsewhere/src/a.rs")),
                Admission::Walked
            )
            .is_none(),
        "and does not match when that form does not either"
    );
}

/// Two configs declaring entries are two origins; a glob is matched
/// against its *own* origin, so one spelling can match under one and
/// not the other, and the outside-origin fallback is per origin too.
// unix: same reason as `a_file_inside_the_origin_is_matched_relative_to_it`.
#[cfg(unix)]
#[test]
fn entries_from_two_configs_are_each_matched_at_their_own_origin() {
    let mut entries = entries(&[("src/**", &[("cyclomatic", 40.0)])]);
    let mut overlay = entry_at(1, "gen/**", "/cfg/sub", &[("cognitive", 50.0)]);
    overlay.source = "sub/overlay.toml".to_owned();
    entries.push(overlay);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    let both = layer
        .for_file(
            None,
            Path::new("/cfg/sub/gen/a.rs"),
            CwdForm(Path::new("sub/gen/a.rs")),
            Admission::Walked,
        )
        .expect("`src/**` by the seed form; `gen/**` relative to /cfg/sub");
    assert_eq!(
        resolved(&both),
        BTreeMap::from([("cyclomatic", 15.0), ("cognitive", 50.0)]),
        "the overlay anchors at its own directory; `src/**` does not match `sub/gen/a.rs` \
         relative to /cfg and the seed form is `sub/gen/a.rs` too"
    );
    assert_eq!(
        layer.overrides_of("cognitive")[0].context,
        "[[thresholds.path]] #1 (glob \"gen/**\") in sub/overlay.toml",
        "the overlay's first entry is #1 of its own file, not #2 of the merged list"
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_path_is_matched_by_bytes_and_never_stringified() {
    use std::os::unix::ffi::OsStrExt;

    let entries = entries(&[("src/**", &[("cyclomatic", 40.0)])]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"src/bad\x80.rs"));
    assert!(
        layer
            .for_file(None, &bad, CwdForm(&bad), Admission::Walked)
            .is_some(),
        "the glob matches the bytes; nothing in the lookup needs UTF-8"
    );
}

/// A file lexically outside every origin reaches the canonicalising
/// fallback. The layer resolves the file's *directory* once and
/// appends the name, so a second file in the same directory — and a
/// directory reached through a symlink — are answered from the memo
/// with the same result a per-file `canonicalize` gave (#1746).
#[cfg(unix)]
#[test]
fn outside_origin_matching_canonicalises_once_per_directory() {
    let td = tempfile::tempdir().expect("tempdir");
    let real = td.path().canonicalize().expect("canonical tempdir");
    std::fs::create_dir_all(real.join("project/src")).expect("mkdir");
    std::fs::write(real.join("project/src/a.rs"), "").expect("write");
    std::fs::write(real.join("project/src/b.rs"), "").expect("write");
    let link = real.join("link");
    std::os::unix::fs::symlink(real.join("project"), &link).expect("symlink");

    // The origin is the real project; the files are spelled through the
    // symlink, so neither lexical strip succeeds and only the canonical
    // form puts them under the origin.
    let entries = [entry_at(
        1,
        "src/*.rs",
        real.join("project").to_str().expect("utf8"),
        &[("cyclomatic", 40.0)],
    )];
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    for name in ["a.rs", "b.rs"] {
        let spelled = link.join("src").join(name);
        assert!(
            layer
                .for_file(
                    None,
                    &spelled,
                    CwdForm(Path::new("elsewhere")),
                    Admission::Walked
                )
                .is_some(),
            "{name}: matched through the symlinked spelling"
        );
    }
    let dirs = layer.canonical_dirs.0.lock().expect("unpoisoned");
    assert_eq!(
        dirs.keys().collect::<Vec<_>>(),
        [&link.join("src")],
        "one directory resolved for two files"
    );
    assert_eq!(
        dirs.values().flatten().collect::<Vec<_>>(),
        [&real.join("project/src")],
    );
}

/// A seeded file that is itself a symlink is matched by its target's
/// path, as it was before the per-directory memo (#1746). The memo
/// resolves the link's *parent* and would otherwise append the link's
/// own name, leaving `alias.c` outside the origin and `src/**` unmatched.
#[cfg(unix)]
#[test]
fn a_seeded_file_symlink_is_matched_by_its_target() {
    let td = tempfile::tempdir().expect("tempdir");
    let real = td.path().canonicalize().expect("canonical tempdir");
    std::fs::create_dir_all(real.join("project/src")).expect("mkdir");
    std::fs::write(real.join("project/src/target.rs"), "").expect("write");
    let alias = real.join("alias.rs");
    std::os::unix::fs::symlink(real.join("project/src/target.rs"), &alias).expect("symlink");

    let entries = [entry_at(
        1,
        "src/*.rs",
        real.join("project").to_str().expect("utf8"),
        &[("cyclomatic", 40.0)],
    )];
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    assert!(
        layer
            .for_file(
                None,
                &alias,
                CwdForm(Path::new("alias.rs")),
                Admission::Explicit
            )
            .is_some(),
        "the link resolves to project/src/target.rs, which src/*.rs matches"
    );
    assert!(
        layer
            .for_file(
                None,
                &alias,
                CwdForm(Path::new("alias.rs")),
                Admission::Walked
            )
            .is_none(),
        "a walked file is never a link, so the probe is skipped and the spelled name stands"
    );
}

/// `Debug` on the layer summarises rather than dumps: the entry count
/// and the base slugs, never the memo or the resolver. It is what
/// `LanguageThresholds`'s derived `Debug` prints for its `path` field,
/// so a failing assertion on the gate shows the shape, not 300 lines.
#[test]
fn debug_summarises_the_layer_as_entry_count_and_bases() {
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0)]),
        ("gen/**", &[("cognitive", 50.0)]),
    ]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    assert_eq!(
        format!("{layer:?}"),
        "PathLayer { entries: 2, bases: [None, Some(\"c\")], .. }"
    );
}

/// Narrowing keeps the memoised sets narrowed and narrows anything
/// resolved afterwards. The second half needs a set that is *not* in
/// the memo at narrowing time: construction resolves every single
/// entry over every base, so a one-entry lookup always hits; the lazy
/// path is reached only by a combination of entries.
#[test]
fn narrowing_keeps_the_memo_narrowed_and_narrows_later_resolutions() {
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0), ("cognitive", 20.0)]),
        ("src/gen/**", &[("cyclomatic", 60.0)]),
    ]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    let narrowed = layer.narrowed_to(&BTreeSet::from(["cognitive"]));
    assert_eq!(narrowed.resolved_sets().len(), 4);
    for set in narrowed.resolved_sets() {
        assert_eq!(
            resolved(&set).keys().copied().collect::<Vec<_>>(),
            ["cognitive"]
        );
    }
    // The two-entry combination is resolved now, after narrowing.
    let c = narrowed
        .for_file(
            Some("c"),
            Path::new("src/gen/a.c"),
            CwdForm(Path::new("src/gen/a.c")),
            Admission::Walked,
        )
        .expect("both match");
    assert_eq!(resolved(&c), BTreeMap::from([("cognitive", 20.0)]));
    assert_eq!(
        narrowed.resolved_sets().len(),
        5,
        "the combination was resolved lazily, not fetched"
    );
}

#[test]
fn overrides_of_reports_each_entry_declaring_the_metric() {
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0)]),
        ("gen/**", &[("cognitive", 50.0)]),
        ("vendor/**", &[("cyclomatic", 80.0)]),
    ]);
    let layer = PathLayer::new(
        &entries,
        bases(),
        Arc::new(Recording(Mutex::new(Vec::new()))),
    );
    let overrides = layer.overrides_of("cyclomatic");
    let seen: Vec<(&str, f64)> = overrides
        .iter()
        .map(|o| (o.context.as_str(), o.hard))
        .collect();
    assert_eq!(
        seen,
        [
            ("[[thresholds.path]] #1 (glob \"src/**\") in bca.toml", 40.0),
            (
                "[[thresholds.path]] #3 (glob \"vendor/**\") in bca.toml",
                80.0
            ),
        ],
        "the cognitive-only entry earns no line for cyclomatic"
    );
}

/// `--print-effective-config` prints each entry's declared metrics at
/// their hard-tier limit — the CLI override applied, the tier not —
/// so the printout read back under the same `--tier` re-derives the
/// same band (#1746). A resolver that applies a CLI override and then
/// halves every limit for its tier stands in for the real layering.
#[test]
fn resolved_rows_carry_the_hard_limits_for_the_declared_metrics_only() {
    struct CliWinsThenHalves;
    impl HardResolver for CliWinsThenHalves {
        fn resolve(&self, mut hard: BTreeMap<String, f64>, _: &str) -> ThresholdSet {
            hard.insert("cyclomatic".to_owned(), 3.0);
            let soft: BTreeMap<String, f64> =
                hard.iter().map(|(k, v)| (k.clone(), v / 2.0)).collect();
            ThresholdSet::build_tiered(&soft, &hard).expect("valid")
        }
    }
    let entries = entries(&[
        ("src/**", &[("cyclomatic", 40.0), ("cognitive", 20.0)]),
        ("gen/**", &[("cognitive", 50.0)]),
    ]);
    let layer = PathLayer::new(&entries, bases(), Arc::new(CliWinsThenHalves));
    let rows: Vec<(&str, BTreeMap<String, f64>)> = layer.resolved_rows().collect();
    assert_eq!(
        rows,
        [
            (
                "src/**",
                limits(&[("cyclomatic", 3.0), ("cognitive", 20.0)])
            ),
            ("gen/**", limits(&[("cognitive", 50.0)])),
        ],
        "the overridden cyclomatic prints at the CLI value and nothing prints halved; the \
         base's own cognitive (15) is not an entry row"
    );
}
