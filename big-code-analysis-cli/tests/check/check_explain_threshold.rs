//! Integration tests for `bca check --explain-threshold` (issue #1169).
//!
//! The feature's whole value is that its counts match the gate run it
//! predicts, so most of what follows is that equality asserted against a
//! *real* `bca check` at the same limit, once per configuration layer
//! that could make the two diverge: `[check] exclude`, `--exclude-tests`,
//! in-source suppression markers, and a baseline.
//!
//! Every fixture is sized so the two tiers disagree and both are
//! non-zero. A preview whose filter selected nothing would otherwise
//! agree with a gate run that also selected nothing, and the whole suite
//! would pass vacuously.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// One Rust function taking `arity` parameters, named `f{id}`.
fn nargs_fn(id: usize, arity: usize) -> String {
    let params: Vec<String> = (0..arity).map(|p| format!("a{p}: u8")).collect();
    format!("pub fn f{id}({}) -> u8 {{ {arity} }}\n", params.join(", "))
}

/// A Rust source file whose `nargs` distribution is exactly `spec`, read
/// as `(arity, how many functions at it)`.
fn nargs_source(spec: &[(usize, usize)]) -> String {
    let mut out = String::new();
    let mut id = 0;
    for (arity, count) in spec {
        for _ in 0..*count {
            out.push_str(&nargs_fn(id, *arity));
            id += 1;
        }
    }
    out
}

/// The fixture every equality test below shares: 20 functions at exactly
/// 4 parameters, 6 at 5, and 3 at 6.
///
/// Against a candidate `nargs = 4` that resolves to 9 hard-tier offenders
/// (`> 4`) and 29 soft-tier ones (`> 3.8`), so the two tiers cannot be
/// confused for one another, and the 20-function soft band sits on one
/// value — the shape the whole issue is about.
const CANDIDATE_SPEC: &[(usize, usize)] = &[(4, 20), (5, 6), (6, 3)];
const CANDIDATE_METRIC: &str = "nargs";
const CANDIDATE_LIMIT: &str = "4";
const EXPECTED_HARD: usize = 9;
const EXPECTED_SOFT: usize = 29;

/// One tier's row, parsed back out of the preview's stdout.
#[derive(Debug, PartialEq, Eq)]
struct Tier {
    limit: String,
    total: usize,
    baselined: usize,
    new: usize,
}

/// The preview report for one metric.
#[derive(Debug)]
struct Preview {
    hard: Tier,
    soft: Tier,
    cluster: Option<String>,
}

/// Parse `  <name> tier (limit <L>[, <derivation>]): N offenders, M
/// already baselined, K new`.
///
/// Splitting on `"): "` rather than on every comma is deliberate: the
/// soft row's limit segment carries the derivation after a comma, so a
/// naive split would shear it and silently mis-assign the counts.
fn parse_tier(line: &str) -> Tier {
    let (limit, counts) = line
        .split_once("): ")
        .expect("tier row has a limit segment");
    let limit = limit
        .split_once("(limit ")
        .expect("tier row names its limit")
        .1
        .to_string();
    let mut fields = counts.split(", ").map(|field| {
        field
            .split_whitespace()
            .next()
            .expect("count field is non-empty")
            .parse::<usize>()
            .expect("count field is a number")
    });
    Tier {
        limit,
        total: fields.next().expect("offender count"),
        baselined: fields.next().expect("baselined count"),
        new: fields.next().expect("new count"),
    }
}

fn parse_preview(stdout: &str, metric: &str) -> Preview {
    let header = format!("{metric}: candidate limit ");
    let start = stdout
        .lines()
        .position(|line| line.starts_with(&header))
        .unwrap_or_else(|| panic!("preview reports {metric}; got:\n{stdout}"));
    // The block runs to the next unindented header, so a multi-metric
    // report cannot leak one metric's rows into another's.
    let block: Vec<&str> = stdout
        .lines()
        .skip(start + 1)
        .take_while(|line| line.starts_with("  "))
        .collect();
    let row = |prefix: &str| {
        let line = block
            .iter()
            .find(|line| line.trim_start().starts_with(prefix))
            .unwrap_or_else(|| panic!("preview reports the {prefix} row; got:\n{stdout}"));
        parse_tier(line)
    };
    Preview {
        hard: row("hard tier"),
        soft: row("soft tier"),
        cluster: block
            .iter()
            .find(|line| line.trim_start().starts_with("cluster: "))
            .map(|line| (*line).trim().to_string()),
    }
}

/// Offender rows for `metric` in a real `bca check` stdout stream.
fn gate_rows(stdout: &str, metric: &str) -> usize {
    let needle = format!(": {metric} = ");
    stdout.lines().filter(|l| l.contains(&needle)).count()
}

fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.assert().success().get_output().stdout.clone();
    String::from_utf8(out).expect("utf8 stdout")
}

/// Run the preview and the two real gate runs it claims to predict, and
/// assert the three agree.
///
/// `extra` is appended to all three invocations, so whatever
/// configuration layer a caller is exercising applies identically to the
/// preview and to the gate.
fn assert_preview_matches_gate(dir: &Path, extra: &[&str]) -> Preview {
    // The gate has no way to express a candidate limit and a proportional
    // soft tier at once: `--threshold` is absolute and never scaled, so
    // the soft side has to come from a config file. That asymmetry is the
    // bug this feature exists for.
    fs::write(
        dir.join("candidate.toml"),
        format!("[thresholds]\n{CANDIDATE_METRIC} = {CANDIDATE_LIMIT}\n"),
    )
    .expect("write candidate config");

    let preview = parse_preview(
        &stdout_of(
            cli(dir)
                .args(["check", "--explain-threshold"])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}"))
                .args(extra),
        ),
        CANDIDATE_METRIC,
    );

    let gate = |tier: &str| {
        gate_rows(
            &stdout_of(
                cli(dir)
                    .args([
                        "check",
                        "--config",
                        "candidate.toml",
                        "--no-fail",
                        "--no-summary",
                        "--no-remediation",
                        tier,
                    ])
                    .args(extra),
            ),
            CANDIDATE_METRIC,
        )
    };

    // `new`, not `total`: a real gate run drops baseline-covered
    // offenders, which the preview keeps in order to report the split.
    // The fixtures below never regress against their baseline, so the
    // gate's surviving rows are exactly the preview's new ones.
    assert_eq!(
        preview.hard.new,
        gate("--tier=hard"),
        "hard-tier preview must match the gate at the same limit"
    );
    assert_eq!(
        preview.soft.new,
        gate("--tier=soft=0.95"),
        "soft-tier preview must match the gate at the same limit"
    );
    preview
}

fn tree(files: &[(&str, String)]) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write fixture");
    }
    dir
}

fn candidate_tree() -> TempDir {
    tree(&[("lib.rs", nargs_source(CANDIDATE_SPEC))])
}

#[test]
fn preview_matches_the_gate_on_a_plain_tree() {
    let dir = candidate_tree();
    let preview = assert_preview_matches_gate(dir.path(), &["--paths", "lib.rs"]);
    // Pin the absolute counts too, so a helper that compared two equally
    // broken numbers would still fail here.
    assert_eq!(preview.hard.total, EXPECTED_HARD);
    assert_eq!(preview.soft.total, EXPECTED_SOFT);
}

#[test]
fn preview_matches_the_gate_under_check_exclude() {
    // The excluded file offends at every arity in the fixture, so a
    // preview that ignored `[check] exclude` would report visibly larger
    // numbers rather than the same ones by luck.
    let dir = tree(&[
        ("lib.rs", nargs_source(CANDIDATE_SPEC)),
        ("generated.rs", nargs_source(CANDIDATE_SPEC)),
    ]);
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\n[check]\nexclude = [\"./generated.rs\"]\n",
    )
    .expect("write manifest");

    let preview = assert_preview_matches_gate(dir.path(), &[]);
    assert_eq!(preview.hard.total, EXPECTED_HARD);
    assert_eq!(preview.soft.total, EXPECTED_SOFT);

    // Seed check: without the exemption the same tree doubles, so the
    // equality above is the exclusion being honoured and not an empty
    // filter agreeing with an empty filter.
    let unexcluded = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    ".",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(unexcluded.hard.total, EXPECTED_HARD * 2);
}

#[test]
fn preview_matches_the_gate_under_exclude_tests() {
    // The `#[cfg(test)]` module offends at every arity; `--exclude-tests`
    // prunes the whole subtree before any metric is computed.
    let mut body = nargs_source(CANDIDATE_SPEC);
    body.push_str("#[cfg(test)]\nmod tests {\n");
    body.push_str(&nargs_source(CANDIDATE_SPEC));
    body.push_str("}\n");
    let dir = tree(&[("lib.rs", body)]);

    let preview =
        assert_preview_matches_gate(dir.path(), &["--paths", "lib.rs", "--exclude-tests"]);
    assert_eq!(preview.hard.total, EXPECTED_HARD);

    // Seed check: the same tree without the flag carries both copies.
    let with_tests = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    "lib.rs",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(with_tests.hard.total, EXPECTED_HARD * 2);
}

#[test]
fn preview_matches_the_gate_under_suppression_markers() {
    // Suppress every 6-parameter function: three hard-tier offenders
    // disappear from both the preview and the gate.
    let mut body = nargs_source(&[(4, 20), (5, 6)]);
    for id in 26..29 {
        write!(
            body,
            "pub fn f{id}(a0: u8, a1: u8, a2: u8, a3: u8, a4: u8, a5: u8) -> u8 {{\n    \
             // bca: suppress(nargs) -- fixture: marker must drop this offender\n    6\n}}\n"
        )
        .expect("write to String");
    }
    let dir = tree(&[("lib.rs", body)]);

    let preview = assert_preview_matches_gate(dir.path(), &["--paths", "lib.rs"]);
    assert_eq!(
        preview.hard.total,
        EXPECTED_HARD - 3,
        "the three marked functions must not be counted"
    );
    assert_eq!(preview.soft.total, EXPECTED_SOFT - 3);

    // Seed check: `--no-suppress` un-silences them, so the marker is what
    // moved the number.
    let raw = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    "lib.rs",
                    "--no-suppress",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(raw.hard.total, EXPECTED_HARD);

    // `--report-suppressed` keeps marker-silenced offenders in the stream
    // for the SARIF document, but the gate still excludes them — so the
    // preview must too, or it would price a candidate against offenders
    // no run would ever fail on.
    let reported = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    "lib.rs",
                    "--report-suppressed",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(reported.hard.total, EXPECTED_HARD - 3);
}

#[test]
fn preview_splits_baselined_debt_from_new_entries() {
    let dir = candidate_tree();
    // A baseline written at the candidate's *hard* limit covers the 9
    // hard offenders and none of the 20 soft-band ones — the exact
    // asymmetry the report exists to surface.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--threshold",
            "nargs=4",
            "--write-baseline",
            "base.toml",
        ])
        .assert()
        .success();

    let preview = assert_preview_matches_gate(
        dir.path(),
        &["--paths", "lib.rs", "--baseline", "base.toml"],
    );

    assert_eq!(preview.hard.total, EXPECTED_HARD);
    assert_eq!(preview.hard.baselined, EXPECTED_HARD);
    assert_eq!(preview.hard.new, 0, "the hard tier reads as free");
    assert_eq!(preview.soft.total, EXPECTED_SOFT);
    assert_eq!(preview.soft.baselined, EXPECTED_HARD);
    assert_eq!(
        preview.soft.new,
        EXPECTED_SOFT - EXPECTED_HARD,
        "and the soft tier is where the candidate's real cost lands"
    );
}

/// A baselined offender whose value has *worsened* still has a baseline
/// entry, so it is counted as baselined rather than as a new one — a
/// refresh would update its record, not add one.
#[test]
fn a_regressed_offender_counts_as_baselined_not_new() {
    let dir = candidate_tree();
    fs::write(
        dir.path().join("base.toml"),
        // `f26` is the first six-parameter function and each fixture
        // function occupies one line, so it starts at line 27. The
        // recorded 5 is below its real 6, which is what makes it a
        // regression rather than covered debt.
        "version = 5\n\n[provenance]\ntier = \"hard\"\n\n[[entry]]\n\
         path = \"lib.rs\"\nqualified = \"f26\"\nstart_line = 27\n\
         metric = \"nargs\"\nvalue = 5.0\n",
    )
    .expect("write baseline");

    let preview = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    "lib.rs",
                    "--baseline",
                    "base.toml",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(preview.hard.total, EXPECTED_HARD);
    assert_eq!(
        preview.hard.baselined, 1,
        "f26 has an entry (recorded 5, now 6) and is therefore not a new entry"
    );
    assert_eq!(preview.hard.new, EXPECTED_HARD - 1);
}

/// The v6 schema omits `start_line` for a unique identity (#1170).
/// `--explain-threshold` resolves its baselined/new split through the
/// same matcher as the gate, so dropping the line must not move a single
/// offender between the two columns — asserted against the byte-for-byte
/// numbers the v5 sibling test above pins.
#[test]
fn a_regressed_offender_counts_as_baselined_without_a_recorded_line() {
    let dir = candidate_tree();
    fs::write(
        dir.path().join("base.toml"),
        // Identical to the v5 fixture above except for the schema stamp
        // and the absent `start_line`: `f26` is a unique identity, so a
        // v6 `--write-baseline` records no line for it.
        "version = 6\n\n[provenance]\ntier = \"hard\"\n\n[[entry]]\n\
         path = \"lib.rs\"\nqualified = \"f26\"\n\
         metric = \"nargs\"\nvalue = 5.0\n",
    )
    .expect("write baseline");

    let preview = parse_preview(
        &stdout_of(
            cli(dir.path())
                .args([
                    "check",
                    "--no-config",
                    "--paths",
                    "lib.rs",
                    "--baseline",
                    "base.toml",
                    "--explain-threshold",
                ])
                .arg(format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}")),
        ),
        CANDIDATE_METRIC,
    );
    assert_eq!(preview.hard.total, EXPECTED_HARD);
    assert_eq!(
        preview.hard.baselined, 1,
        "f26 still matches its entry with no line recorded"
    );
    assert_eq!(preview.hard.new, EXPECTED_HARD - 1);
}

/// The soft tier is derived direction-aware (#1166): a lower-is-worse
/// `mi.*` limit is a floor, so tightening it raises it.
#[test]
fn soft_limit_is_derived_by_metric_direction() {
    let dir = candidate_tree();
    let stdout = stdout_of(cli(dir.path()).args([
        "check",
        "--no-config",
        "--paths",
        "lib.rs",
        "--explain-threshold",
        "mi.original=20",
        "--explain-threshold",
        "nargs=4",
    ]));
    let floor = parse_preview(&stdout, "mi.original");
    let ceiling = parse_preview(&stdout, "nargs");
    assert_eq!(floor.hard.limit, "20");
    assert_eq!(
        floor.soft.limit, "21.0527, 0.95x",
        "a lower-is-worse floor is tightened by raising it"
    );
    assert_eq!(ceiling.hard.limit, "4");
    assert_eq!(
        ceiling.soft.limit, "3.8, 0.95x",
        "a higher-is-worse ceiling is tightened by lowering it"
    );
}

/// Direction decides which *offenders* land in the hard tier, not only
/// how the soft limit is derived — and that is a second production
/// expression, `breaches_limit(v.value, ceiling, v.lower_is_worse)`.
///
/// The test above cannot reach it: `mi.original=20` is a floor that every
/// one-line fixture function (all measure ≈147-149) clears, so its
/// population is empty at both tiers and the partition never runs.
/// Hard-coding `false` there — the pre-#1166 "higher is worse" assumption
/// — failed none of the suite's 5053 tests.
///
/// A floor of `147.5` splits the fixture's three measured values
/// (146.9456 ×3, 147.7445 ×6, 148.655 ×20): three sit below it and
/// breach, all 29 sit below the derived `155.264` soft floor. Read with
/// the direction inverted the hard tier would instead collect the 26
/// *above* 147.5, so the two readings share no count.
///
/// Both metrics are explained in one run, and both carry a *non-empty*
/// population — which is the second thing this pins. Every other
/// multi-metric test here pairs a real population with an empty one, so
/// `explain`'s per-metric filter (`v.metric == outcome.metric`) could be
/// deleted outright without failing any of the suite's 5054 tests. With
/// two live populations each metric would then tally the union: 12 and
/// 58 everywhere, against the four distinct counts below.
#[test]
fn a_lower_is_worse_metric_partitions_offenders_by_direction() {
    let dir = candidate_tree();
    let candidate = format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}");
    let stdout = stdout_of(cli(dir.path()).args([
        "check",
        "--no-config",
        "--paths",
        "lib.rs",
        "--explain-threshold",
        "mi.original=147.5",
        "--explain-threshold",
        &candidate,
    ]));
    let floor = parse_preview(&stdout, "mi.original");
    assert_eq!(
        floor.hard.total, 3,
        "only the three functions *below* the floor breach it; \
         reading it as a ceiling would report 26. stdout:\n{stdout}"
    );
    assert_eq!(floor.hard.new, 3, "none of them is baselined");
    assert_eq!(
        floor.soft.total, 29,
        "the whole population sits under the raised soft floor; stdout:\n{stdout}"
    );

    // The higher-is-worse metric keeps its own counts in the same run.
    let ceiling = parse_preview(&stdout, CANDIDATE_METRIC);
    assert_eq!(ceiling.hard.total, EXPECTED_HARD);
    assert_eq!(ceiling.soft.total, EXPECTED_SOFT);
}

/// The candidate limit the two soft-derivation tests below share. It
/// sits above every arity in the fixture, so the *default* 0.95 band is
/// empty and any offender reported at the soft tier can only come from
/// a soft limit the run derived some other way.
const LOOSE_LIMIT: &str = "nargs=8";

/// `--tier=soft=<R>` pins the ratio the preview scales its soft limit
/// by. Without it the report falls back to `DEFAULT_SOFT_HEADROOM`,
/// which is the only path the rest of this suite exercises.
///
/// Both the derived limit and the offender count move with the ratio,
/// so a preview that ignored the flag could not pass this by luck.
#[test]
fn an_explicit_soft_ratio_derives_the_soft_limit() {
    let dir = candidate_tree();
    let preview_with = |tier: &[&str]| {
        parse_preview(
            &stdout_of(
                cli(dir.path())
                    .args([
                        "check",
                        "--no-config",
                        "--paths",
                        "lib.rs",
                        "--explain-threshold",
                        LOOSE_LIMIT,
                    ])
                    .args(tier),
            ),
            CANDIDATE_METRIC,
        )
    };

    let explicit = preview_with(&["--tier=soft=0.5"]);
    assert_eq!(
        explicit.soft.limit, "4, 0.5x",
        "the supplied ratio scales the candidate, not 0.95"
    );
    assert_eq!(
        explicit.soft.total, 9,
        "the six 5-parameter and three 6-parameter functions breach a soft limit of 4"
    );
    assert_eq!(explicit.hard.total, 0, "and none of them breach 8");

    // Seed check: the same tree at the default tier derives 7.6, which
    // nothing in the fixture reaches — so the ratio is what moved both
    // the limit and the count.
    let default = preview_with(&[]);
    assert_eq!(default.soft.limit, "7.6, 0.95x");
    assert_eq!(default.soft.total, 0);
}

/// A `[thresholds.soft]` entry for the explained metric supplies the
/// soft limit outright. The report must attribute it to that table
/// rather than to a ratio it never applied.
#[test]
fn a_soft_table_entry_is_named_as_the_soft_limits_source() {
    let dir = candidate_tree();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\"lib.rs\"]\n[thresholds.soft]\nnargs = 4\n",
    )
    .expect("write manifest");

    let preview = parse_preview(
        &stdout_of(cli(dir.path()).args(["check", "--explain-threshold", LOOSE_LIMIT])),
        CANDIDATE_METRIC,
    );
    assert_eq!(
        preview.soft.limit, "4, [thresholds.soft]",
        "the table's own value, attributed to the table"
    );
    assert_eq!(preview.soft.total, 9);
    assert_eq!(preview.hard.total, 0);

    // Seed check: drop the manifest and the same candidate derives its
    // soft limit from the ratio instead, so the attribution above is the
    // table being consulted and not a constant that happens to fit.
    let ratio_derived = parse_preview(
        &stdout_of(cli(dir.path()).args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            LOOSE_LIMIT,
        ])),
        CANDIDATE_METRIC,
    );
    assert_eq!(ratio_derived.soft.limit, "7.6, 0.95x");
}

/// The third way a soft limit is arrived at, and the one the report
/// used to misattribute. `resolve_tier`'s soft branch is all-or-nothing
/// per table: a single `[thresholds.soft]` entry switches the whole
/// table into merge mode, so an explained metric the table does *not*
/// name keeps its hard limit with no ratio applied to it at all.
///
/// The counts were always right; the annotation was not. `limit 8,
/// 0.95x` says 8 × 0.95, which is 7.6 — a number that appears nowhere
/// in the run, on the one command whose entire purpose is to be trusted
/// for a threshold decision.
#[test]
fn a_metric_the_soft_table_omits_reports_no_soft_band() {
    // A candidate the fixture straddles at *both* tiers: 3 functions
    // exceed 5, and 9 exceed the 4.75 a ratio would derive. A candidate
    // above the fixture's whole range would compare 0 against 0 and the
    // contrast below would hold vacuously.
    const TIGHT_LIMIT: &str = "nargs=5";
    const INHERITED_OFFENDERS: usize = 3;
    const RATIO_OFFENDERS: usize = 9;

    let dir = candidate_tree();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\"lib.rs\"]\n[thresholds.soft]\ncognitive = 4\n",
    )
    .expect("write manifest");

    let stdout = stdout_of(cli(dir.path()).args([
        "check",
        "--explain-threshold",
        TIGHT_LIMIT,
        "--explain-threshold",
        "cognitive=10",
    ]));

    // The metric the table names still reports the table.
    assert_eq!(
        parse_preview(&stdout, "cognitive").soft.limit,
        "4, [thresholds.soft]",
    );

    // The metric it omits inherits the candidate verbatim, and says so.
    let nargs = parse_preview(&stdout, CANDIDATE_METRIC);
    assert_eq!(
        nargs.soft.limit,
        "5, no soft band; [thresholds.soft] names other metrics, \
         so the hard limit stands",
    );
    // Same limit at both tiers means the same offenders at both — the
    // fact the annotation has to stay consistent with.
    assert_eq!(nargs.hard.total, INHERITED_OFFENDERS);
    assert_eq!(nargs.soft.total, INHERITED_OFFENDERS);

    // Seed check: drop the soft table and the identical candidate does
    // get a ratio-derived band, over a strictly larger population. The
    // reported line therefore tracks a real difference rather than a
    // constant that happens to fit.
    let ratio_derived = parse_preview(
        &stdout_of(cli(dir.path()).args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            TIGHT_LIMIT,
        ])),
        CANDIDATE_METRIC,
    );
    assert_eq!(ratio_derived.soft.limit, "4.75, 0.95x");
    assert_eq!(ratio_derived.soft.total, RATIO_OFFENDERS);
}

#[test]
fn cluster_fires_when_the_soft_band_sits_on_the_candidate_limit() {
    let dir = candidate_tree();
    let preview = parse_preview(
        &stdout_of(cli(dir.path()).args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "nargs=4",
        ])),
        CANDIDATE_METRIC,
    );
    let cluster = preview
        .cluster
        .expect("a converged limit reports a cluster");
    assert!(
        cluster.contains("20 of 20 soft-band offenders sit at exactly 4"),
        "cluster line names the population and the value: {cluster}"
    );
    assert!(
        cluster.contains("the candidate limit itself"),
        "and says the limit converged onto it: {cluster}"
    );
}

#[test]
fn cluster_stays_quiet_when_the_soft_band_is_dispersed() {
    // `loc.ploc` counts one physical line per statement here, so each
    // file's value is its line count. Fifteen files spread evenly over
    // the five values in the (95, 100] band leave no value above the
    // majority share, while the band is comfortably over the
    // ten-function floor — so this exercises the *share* rule, not the
    // size one.
    let mut files = Vec::new();
    for ploc in 96..=100_usize {
        for copy in 0..3 {
            let mut body = String::new();
            for i in 0..ploc {
                writeln!(body, "fn f{i}() {{ }}").expect("write to String");
            }
            files.push((format!("f{ploc}_{copy}.rs"), body));
        }
    }
    let borrowed: Vec<(&str, String)> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.clone()))
        .collect();
    let dir = tree(&borrowed);

    let preview = parse_preview(
        &stdout_of(cli(dir.path()).args([
            "check",
            "--no-config",
            "--paths",
            ".",
            "--explain-threshold",
            "loc.ploc=100",
        ])),
        "loc.ploc",
    );
    assert_eq!(
        preview.soft.total, 15,
        "all fifteen files fall inside the (95, 100] soft band"
    );
    assert_eq!(preview.hard.total, 0, "and none of them breach 100");
    assert_eq!(
        preview.cluster, None,
        "no single value holds a majority of a dispersed band"
    );
}

/// The preview is a report, not a gate: it exits 0 even over a tree the
/// candidate limit would fail, and says so.
#[test]
fn preview_exits_zero_over_an_offending_tree() {
    let dir = candidate_tree();
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "nargs=4",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "no gate ran and the exit code is always 0",
        ));
}

/// Stream contract (#1167): the report is this invocation's product, so
/// it belongs on stdout and the diagnostics stay on stderr.
#[test]
fn report_goes_to_stdout_and_diagnostics_to_stderr() {
    let dir = candidate_tree();
    let assert = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "nargs=4",
        ])
        .assert()
        .success();
    let out = assert.get_output();
    let stdout = String::from_utf8(out.stdout.clone()).expect("utf8 stdout");
    let stderr = String::from_utf8(out.stderr.clone()).expect("utf8 stderr");

    assert!(stdout.contains("nargs: candidate limit 4"), "{stdout}");
    assert!(stdout.contains("hard tier (limit 4)"), "{stdout}");
    assert!(stdout.contains("cluster: "), "{stdout}");
    assert!(
        !stderr.contains("candidate limit"),
        "the report must not be duplicated onto stderr: {stderr}"
    );
    assert!(
        stderr.contains("bca: --explain-threshold is a preview"),
        "the preview caveat is a diagnostic: {stderr}"
    );
}

/// A per-language override of the explained metric keeps its own limit,
/// and the report says so rather than reporting a number that would not
/// apply there.
#[test]
fn per_language_override_of_the_explained_metric_is_reported() {
    let dir = tree(&[
        ("lib.rs", nargs_source(CANDIDATE_SPEC)),
        (
            "wide.c",
            "int wide(int a, int b, int c, int d, int e) { return a; }\n".to_string(),
        ),
    ]);
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\n[thresholds.lang.c]\nnargs = 8\n",
    )
    .expect("write manifest");

    let stdout = stdout_of(cli(dir.path()).args(["check", "--explain-threshold", "nargs=4"]));
    let preview = parse_preview(&stdout, CANDIDATE_METRIC);
    assert!(
        stdout.contains("[thresholds.lang.c] keeps nargs at 8 (soft 7.6)"),
        "the report names the language keeping its own limit: {stdout}"
    );
    // The C function has five parameters and so would offend at the
    // candidate, but its language gates at 8 — the count must exclude it.
    assert_eq!(preview.soft.total, EXPECTED_SOFT);
}

/// Two ways the request can contradict itself, both rejected rather than
/// silently resolved.
#[test]
fn contradictory_candidates_are_rejected() {
    let dir = candidate_tree();
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "nargs=4",
            "--explain-threshold",
            "nargs=5",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "preview one candidate limit per metric per run",
        ));

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "nargs=4",
            "--threshold",
            "nargs=6",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "a --threshold limit is absolute and has no soft tier to preview",
        ));
}

/// Every flag the preview refuses to share a run with, and the reason
/// each is refused: the preview returns before `emit_check_results`, so
/// a second artifact the user asked for would silently never be
/// written. Without this the whole `conflicts_with_all` list is
/// unguarded — dropping `"output"` from it would let
/// `--explain-threshold X --output f.sarif` run the preview and produce
/// no document, with nothing failing.
#[test]
fn flags_producing_a_second_artifact_are_rejected() {
    let dir = candidate_tree();
    let candidate = format!("{CANDIDATE_METRIC}={CANDIDATE_LIMIT}");
    let out = dir.path().join("artifact.out");
    let out = out.to_str().expect("utf-8 tempdir path");
    for extra in [
        "--write-baseline",
        "--print-effective-config=json",
        "--report-format=checkstyle",
        &format!("--output={out}"),
    ] {
        cli(dir.path())
            .args([
                "check",
                "--no-config",
                "--paths",
                "lib.rs",
                "--explain-threshold",
                &candidate,
                extra,
            ])
            .assert()
            .code(1)
            // clap names both sides; asserting only the exit code would
            // pass on any unrelated usage error.
            .stderr(predicate::str::contains("--explain-threshold"))
            .stderr(predicate::str::contains(
                extra.split('=').next().unwrap_or(extra),
            ));
        assert!(
            !std::path::Path::new(out).exists(),
            "a rejected run must not have written {out}",
        );
    }

    // `--summary-file <path>` is the same silent no-op but cannot ride
    // on `conflicts_with_all`, which fires on the flag's presence and
    // would take the keyword forms with it.
    let summary = dir.path().join("summary.md");
    let summary = summary.to_str().expect("utf-8 tempdir path");
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            &candidate,
            "--summary-file",
            summary,
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--explain-threshold cannot be used with --summary-file",
        ));
    assert!(
        !std::path::Path::new(summary).exists(),
        "a rejected run must not have written {summary}",
    );

    // …while `auto` keeps working: it names no destination of its own,
    // so the preview producing no step summary is what every other
    // non-gating run does. Rejecting it would break any CI invocation
    // that passes the keyword explicitly.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            &candidate,
            "--summary-file",
            "auto",
        ])
        .assert()
        .code(0);
}

/// The two typo classes `--threshold` rejects, routed through the same
/// `normalize_for_check` + `build_tiered` pair — and each message must
/// name the flag the user actually passed. `canonical_cli_thresholds`
/// hardcoded `--threshold` for all three of its call sites, so an
/// ambiguous candidate blamed a flag that was never on the command line.
#[test]
fn an_unusable_candidate_metric_is_rejected_naming_this_flag() {
    let dir = candidate_tree();
    // A family head with no single scalar: rejected before the walk, by
    // the name-resolution layer that owns the flag label.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "halstead=5",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--explain-threshold: ambiguous metric \"halstead\"",
        ))
        .stderr(predicate::str::contains("halstead.effort"))
        // The flag the user did not pass must not be blamed.
        .stderr(predicate::str::contains("--threshold:").not());

    // An unknown name: rejected by the threshold builder, with the
    // did-you-mean vocabulary.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "lib.rs",
            "--explain-threshold",
            "not_a_metric=1",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "unknown threshold metric \"not_a_metric\"",
        ));
}
