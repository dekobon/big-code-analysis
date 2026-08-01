//! Integration tests for `[thresholds.lang.<slug>]` per-language
//! threshold overrides (issue #1141).
//!
//! The fixtures span the three ends of the measured per-language spread
//! the feature exists for — C (loosest), C# (tightest), Elixir (whose
//! `defmodule` is a Container holding many functions, not a class) —
//! plus Rust as a language nobody overrides. Every metric value
//! asserted below was measured with `bca check --threshold <m>=0`
//! against these exact fixtures, not estimated.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// The offender lines from a `bca check` stderr stream, isolated from
/// the summary and remediation blocks.
///
/// Filtering matters here: the remediation footer echoes the resolved
/// `--paths` list, so a bare `stderr.contains("branchy.c")` reads as an
/// offender even when C was gated clean — the precise false pass these
/// tests exist to rule out.
fn offenders(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.contains(" (limit "))
        .collect()
}

/// C: `cognitive = 6`, `cyclomatic = 7`.
const BRANCHY_C: &str = "int classify(int n) {
    if (n < 0) { return -1; }
    if (n == 0) { return 0; }
    if (n < 10) { return 1; }
    if (n < 100) { return 2; }
    if (n < 1000) { return 3; }
    if (n < 10000) { return 4; }
    return 5;
}
";

/// C: `cognitive = 12` — twice `BRANCHY_C`, so the two straddle a limit
/// set between them.
const WIDE_C: &str = "int wide(int n) {
    if (n == 1) { return 1; }
    if (n == 2) { return 2; }
    if (n == 3) { return 3; }
    if (n == 4) { return 4; }
    if (n == 5) { return 5; }
    if (n == 6) { return 6; }
    if (n == 7) { return 7; }
    if (n == 8) { return 8; }
    if (n == 9) { return 9; }
    if (n == 10) { return 10; }
    if (n == 11) { return 11; }
    if (n == 12) { return 12; }
    return 0;
}
";

/// Rust: `cognitive = 6`, `cyclomatic = 7` — deliberately the same
/// scores as `BRANCHY_C`, so a difference in outcome can only come from
/// the language, never from the code.
const BRANCHY_RUST: &str = "pub fn classify(n: i32) -> i32 {
    if n < 0 { return -1; }
    if n == 0 { return 0; }
    if n < 10 { return 1; }
    if n < 100 { return 2; }
    if n < 1000 { return 3; }
    if n < 10000 { return 4; }
    5
}
";

/// C#: `cognitive = 6`, `cyclomatic = 7` on `Sample::Classify`.
const BRANCHY_CSHARP: &str = "public class Sample
{
    public int Classify(int n)
    {
        if (n < 0) { return -1; }
        if (n == 0) { return 0; }
        if (n < 10) { return 1; }
        if (n < 100) { return 2; }
        if (n < 1000) { return 3; }
        if (n < 10000) { return 4; }
        return 5;
    }
}
";

/// Elixir: the `Sample` module is a Container space with `nom = 4`.
const MODULE_ELIXIR: &str = "defmodule Sample do
  def one(a), do: a + 1
  def two(a), do: a + 2
  def three(a), do: a + 3
  def four(a), do: a + 4
end
";

/// Write the polyglot fixture tree plus a `bca.toml` carrying
/// `thresholds_toml`, and return the directory. `cli` anchors the
/// process cwd here, so the manifest is auto-discovered exactly as it
/// would be at a repo root.
fn polyglot_tree(thresholds_toml: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    for (name, body) in [
        ("branchy.c", BRANCHY_C),
        ("wide.c", WIDE_C),
        ("branchy.rs", BRANCHY_RUST),
        ("Sample.cs", BRANCHY_CSHARP),
        ("sample.ex", MODULE_ELIXIR),
    ] {
        fs::write(dir.path().join(name), body).expect("write fixture");
    }
    fs::write(dir.path().join("bca.toml"), thresholds_toml).expect("write manifest");
    dir
}

/// An override applies to its own language and to no other. Both
/// fixtures score `cyclomatic = 7`, so only the language differs.
#[test]
fn override_applies_to_its_language_only() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\", \"branchy.rs\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.c]\n\
         cyclomatic = 10\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let offenders = offenders(&stderr);
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].contains("branchy.rs") && offenders[0].ends_with("cyclomatic = 7 (limit 5)"),
        "Rust keeps the global limit of 5: {offenders:?}"
    );
}

/// A per-language table overrides *per metric* and inherits the rest.
/// C raises `cyclomatic` only, so its `cognitive` still gates at the
/// project limit — and the reported limit proves which table won.
#[test]
fn unoverridden_metric_inherits_the_global_limit() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         cognitive = 4\n\
         [thresholds.lang.c]\n\
         cyclomatic = 10\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let offenders = offenders(&stderr);
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].ends_with("classify: cognitive = 6 (limit 4)"),
        "cognitive inherits the global 4: {offenders:?}"
    );
}

/// The structural cases from #1140 that an override *corrects* rather
/// than tunes: an Elixir `defmodule` is a Container holding many
/// functions, so `nom` needs a module-sized limit, while C# wants a
/// tighter one than the project default.
#[test]
fn corrective_overrides_at_both_ends_of_the_spread() {
    let dir = polyglot_tree(
        "paths = [\"sample.ex\", \"Sample.cs\"]\n\
         [thresholds]\n\
         nom = 3\n\
         cognitive = 20\n\
         [thresholds.lang.elixir]\n\
         nom = 100\n\
         [thresholds.lang.csharp]\n\
         cognitive = 4\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let offenders = offenders(&stderr);
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].ends_with("Sample::Classify: cognitive = 6 (limit 4)"),
        "C# gates at its tightened limit; the Elixir module's nom = 4 sits \
         under its raised limit of 100: {offenders:?}"
    );
}

/// A metric no global table mentions still gates the language that
/// names it.
///
/// The check walk computes only the metric families its thresholds read
/// (#1113). Derive that selection from the global set alone and `nom` is
/// never computed here, so the Elixir module's `nom = 4` reads as the
/// zero default and the gate passes — silently, with no offender and no
/// warning.
///
/// The global limit is deliberately `cyclomatic`, whose dependency set
/// is empty. An earlier draft used `cognitive`, which pulls in `Nom` via
/// `Metric::dependencies` — so `nom` was computed regardless and the
/// test passed against a build with no union at all.
#[test]
fn a_metric_only_a_language_table_gates_is_still_computed() {
    let dir = polyglot_tree(
        "paths = [\"sample.ex\"]\n\
         [thresholds]\n\
         cyclomatic = 20\n\
         [thresholds.lang.elixir]\n\
         nom = 3\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let offenders = offenders(&stderr);
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].ends_with("Sample: nom = 4 (limit 3)"),
        "nom must be computed for Elixir even though no global limit names it: {offenders:?}"
    );
}

/// An unknown slug is a tool error (exit 1) with a did-you-mean hint —
/// never a silent no-op that leaves the author believing a gate moved.
#[test]
fn unknown_slug_is_a_hard_error() {
    let dir = polyglot_tree(
        "paths = [\"branchy.rs\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.rustlang]\n\
         cyclomatic = 10\n",
    );

    cli(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "unknown language \"rustlang\" in [thresholds.lang]",
        ))
        .stderr(predicate::str::contains("did you mean `rust`?"));
}

/// An unknown *metric* inside a language table names that table, so the
/// author does not go hunting through the global `[thresholds]`.
#[test]
fn unknown_metric_in_a_language_table_names_the_table() {
    let dir = polyglot_tree(
        "paths = [\"branchy.rs\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.c]\n\
         cyclomatick = 10\n",
    );

    cli(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "[thresholds.lang.c]: unknown threshold metric \"cyclomatick\"",
        ))
        .stderr(predicate::str::contains("did you mean `cyclomatic`?"));
}

/// A language nobody overrode is gated by the global table through the
/// same fallback that serves an unrecognised file language: one code
/// path, no per-language special case. (A file whose *extension* maps to
/// no grammar never reaches the gate at all — the walk skips it before
/// dispatch — so the fallback is what covers every language the tool
/// does analyse.)
#[test]
fn language_without_an_override_uses_the_global_table() {
    let dir = polyglot_tree(
        "paths = [\"Sample.cs\", \"unknown.zzz\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.c]\n\
         cyclomatic = 100\n",
    );
    fs::write(dir.path().join("unknown.zzz"), "nothing parses this\n").expect("write fixture");

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let offenders = offenders(&stderr);
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].ends_with("Sample::Classify: cyclomatic = 7 (limit 5)"),
        "C# falls through to the global limit: {offenders:?}"
    );
    assert!(
        stderr.contains("skipping explicitly-named file with unrecognized language"),
        "an unrecognised file language never reaches the gate at all: {stderr}"
    );
}

/// `--threshold` is applied last and absolutely, so it outranks a
/// per-language table too — otherwise a command-line limit would be
/// silently inert for exactly the languages a project tuned.
#[test]
fn cli_threshold_outranks_a_language_override() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.c]\n\
         cyclomatic = 100\n",
    );

    cli(dir.path())
        .args(["check", "--threshold", "cyclomatic=6"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "classify: cyclomatic = 7 (limit 6)",
        ));
}

/// `--print-effective-config` emits one fully resolved table per
/// overridden language — inherited limits included, not a diff — and the
/// result is still valid TOML that `--config` can read back.
#[test]
fn print_effective_config_renders_resolved_per_language_tables() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         cognitive = 4\n\
         [thresholds.lang.c]\n\
         cyclomatic = 10\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--print-effective-config", "toml"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");

    let parsed: toml::Table = toml::from_str(&stdout).expect("effective config is valid TOML");
    let thresholds = parsed["thresholds"]
        .as_table()
        .expect("[thresholds] is a table");
    assert_eq!(thresholds["cyclomatic"].as_float(), Some(5.0));
    assert_eq!(thresholds["cognitive"].as_float(), Some(4.0));

    let c = thresholds["lang"]["c"]
        .as_table()
        .expect("[thresholds.lang.c] is a table");
    assert_eq!(c["cyclomatic"].as_float(), Some(10.0), "the override");
    assert_eq!(
        c["cognitive"].as_float(),
        Some(4.0),
        "inherited limits are printed too, not left for the reader to infer"
    );
    assert_eq!(c.len(), 2, "exactly the resolved set: {c:?}");

    // The documented "pipe it back through `--config`" contract, end to
    // end. `--config` is a second entry point into
    // `split_thresholds_table`, distinct from manifest discovery, so this
    // is the only test that would notice the `lang` layer being honoured
    // on one path and dropped on the other.
    let echoed = dir.path().join("effective.toml");
    fs::write(&echoed, &stdout).expect("write effective config");
    let assert = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "branchy.c",
            "--config",
            echoed.to_str().expect("utf8 path"),
            "--print-effective-config",
            "toml",
        ])
        .assert()
        .success();
    let reparsed = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let reparsed: toml::Table = toml::from_str(&reparsed).expect("round-tripped config is TOML");
    assert_eq!(
        reparsed["thresholds"], parsed["thresholds"],
        "the resolved thresholds must survive a --config round trip"
    );
}

/// The soft tier derives from each language's *resolved* hard limit.
///
/// This is the case a globally-derived soft tier gets wrong, and gets
/// wrong silently. With `[thresholds] cognitive = 4`,
/// `[thresholds.lang.c] cognitive = 10`, and `--tier=soft=0.5`, C's soft
/// band is `5`. A C function at cognitive 6 breaches that band while
/// sitting well under C's own hard limit of 10 — an encroachment, exit
/// 2. Compare against the *global* ceiling of 4 instead and 6 reads as a
/// hard breach, exit 5, for every C function between 4 and 10.
#[test]
fn soft_tier_derives_from_the_language_hard_limit() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 4\n\
         [thresholds.lang.c]\n\
         cognitive = 10\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft=0.5", "--exit-codes=tiered"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "classify: cognitive = 6 (limit 5)",
        ));
}

/// The other half of the same contract: a value past the language's own
/// hard limit *is* a hard breach (exit 5). Without this, the test above
/// would pass equally against a build that never escalates at all.
#[test]
fn soft_tier_still_escalates_past_the_language_hard_limit() {
    let dir = polyglot_tree(
        "paths = [\"wide.c\"]\n\
         [thresholds]\n\
         cognitive = 4\n\
         [thresholds.lang.c]\n\
         cognitive = 10\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft=0.5", "--exit-codes=tiered"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("wide: cognitive = 12 (limit 5)"));
}

/// At the soft tier each table is resolved against its *own* hard
/// limits: an inherited limit stays the language's, and a global
/// `[thresholds.soft]` override applies on top of it.
///
/// Note what this test does *not* try to prove. A general
/// `soft <= hard` loop over these tables cannot fail: a scale factor is
/// bounded to `(0, 1]` at parse time, and the one shape that could
/// invert the tiers — an absolute soft value above a language's
/// tightened hard limit — is rejected at resolution, so it never reaches
/// a printed config to assert on. That rejection is pinned by
/// `soft_limit_looser_than_a_language_hard_limit_is_rejected`; this test
/// pins the numbers each table actually resolves to.
#[test]
fn soft_tier_resolves_each_table_against_its_own_hard_limits() {
    let manifest = "paths = [\"branchy.c\"]\n\
                    [thresholds]\n\
                    cognitive = 4\n\
                    cyclomatic = 8\n\
                    [thresholds.soft]\n\
                    cyclomatic = 6\n\
                    [thresholds.lang.c]\n\
                    cognitive = 10\n\
                    [thresholds.lang.csharp]\n\
                    cognitive = 2\n";

    let read = |args: &[&str]| -> toml::Table {
        let dir = polyglot_tree(manifest);
        let assert = cli(dir.path()).args(args).assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
        toml::from_str(&stdout).expect("effective config is valid TOML")
    };

    let hard = read(&["check", "--print-effective-config", "toml"]);
    let soft = read(&["check", "--print-effective-config", "toml", "--tier=soft"]);

    // `cognitive` has no soft override, so it inherits each table's own
    // hard limit — C keeps 10, not the project's 4.
    assert_eq!(
        soft["thresholds"]["lang"]["c"]["cognitive"].as_float(),
        Some(10.0)
    );
    assert_eq!(
        soft["thresholds"]["lang"]["csharp"]["cognitive"].as_float(),
        Some(2.0)
    );
    // The absolute soft override applies to every table, and 6 is below
    // the hard 8 each of them inherits.
    assert_eq!(soft["thresholds"]["cyclomatic"].as_float(), Some(6.0));
    assert_eq!(
        soft["thresholds"]["lang"]["c"]["cyclomatic"].as_float(),
        Some(6.0)
    );

    // The hard tier is unchanged by any of this: each table still
    // carries both metrics at their un-scaled limits, so the soft
    // numbers above are a tier difference and not a lost override.
    let hard_c = hard["thresholds"]["lang"]["c"]
        .as_table()
        .expect("[thresholds.lang.c] is a table");
    assert_eq!(hard_c["cognitive"].as_float(), Some(10.0));
    assert_eq!(hard_c["cyclomatic"].as_float(), Some(8.0));
}

/// A soft limit looser than the hard limit it warns about is rejected,
/// naming the table that produced the clash.
///
/// Per-language overrides make this easy to hit by accident: an absolute
/// `[thresholds.soft]` value is written once against the project limit,
/// then a language *tightens* its hard limit below it. The soft gate
/// would then stay silent while the hard gate fires, and any offender
/// that did trip the soft band would exceed the ceiling too and escalate
/// straight to exit 5.
#[test]
fn soft_limit_looser_than_a_language_hard_limit_is_rejected() {
    let dir = polyglot_tree(
        "paths = [\"Sample.cs\"]\n\
         [thresholds]\n\
         cognitive = 15\n\
         [thresholds.soft]\n\
         cognitive = 12\n\
         [thresholds.lang.csharp]\n\
         cognitive = 4\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("[thresholds.lang.csharp]"))
        .stderr(predicate::str::contains(
            "soft limit 12 is looser than the hard limit 4",
        ));
}

/// A scale-relative soft limit resolves against whichever table supplies
/// the hard limit, even when that is only a language table.
///
/// `[thresholds.soft] nom = "0.9x"` has nothing to scale in the global
/// table here. Resolving each table in isolation would fail the whole run
/// with "no hard `[thresholds]` limit exists for `nom`" against a
/// manifest that plainly defines one.
#[test]
fn scale_relative_soft_resolves_against_a_language_only_hard_limit() {
    let dir = polyglot_tree(
        "paths = [\"sample.ex\"]\n\
         [thresholds]\n\
         cyclomatic = 20\n\
         [thresholds.soft]\n\
         nom = \"0.5x\"\n\
         [thresholds.lang.elixir]\n\
         nom = 6\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--print-effective-config", "toml", "--tier=soft"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let parsed: toml::Table = toml::from_str(&stdout).expect("effective config is valid TOML");

    let thresholds = parsed["thresholds"]
        .as_table()
        .expect("[thresholds] is a table");
    assert!(
        !thresholds.contains_key("nom"),
        "the global table has no nom limit for the scale to apply to: {thresholds:?}"
    );
    assert_eq!(
        thresholds["lang"]["elixir"]["nom"].as_float(),
        Some(3.0),
        "Elixir's nom scales from its own hard 6"
    );
}

/// A scale-relative soft limit that *no* table can supply a base for is
/// still the long-standing hard error — the language-aware lookup must
/// not turn a genuinely orphaned entry into a silent drop.
#[test]
fn scale_relative_soft_with_no_hard_limit_anywhere_still_errors() {
    let dir = polyglot_tree(
        "paths = [\"sample.ex\"]\n\
         [thresholds]\n\
         cyclomatic = 20\n\
         [thresholds.soft]\n\
         nom = \"0.5x\"\n\
         [thresholds.lang.elixir]\n\
         cognitive = 6\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "uses scale-relative syntax but no hard [thresholds] limit exists",
        ));
}

/// `--config` merges into a per-language table per metric, leaving that
/// language's other overrides alone — the same rule the global table
/// follows, one level deeper.
#[test]
fn config_merges_into_a_language_table_per_metric() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 4\n\
         [thresholds.lang.c]\n\
         cognitive = 30\n\
         cyclomatic = 40\n",
    );
    let overlay = dir.path().join("tighten.toml");
    fs::write(&overlay, "[thresholds.lang.c]\ncognitive = 5\n").expect("write overlay");

    let assert = cli(dir.path())
        .args([
            "check",
            "--print-effective-config",
            "toml",
            "--config",
            overlay.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let parsed: toml::Table = toml::from_str(&stdout).expect("effective config is valid TOML");

    let c = parsed["thresholds"]["lang"]["c"]
        .as_table()
        .expect("[thresholds.lang.c] is a table");
    assert_eq!(c["cognitive"].as_float(), Some(5.0), "--config wins");
    assert_eq!(
        c["cyclomatic"].as_float(),
        Some(40.0),
        "a metric --config did not mention keeps the manifest override"
    );
}

/// Gating only a language, with no global `[thresholds]`, is legal and
/// says so out loud.
///
/// It is the one shape where a `bca check` run can exit 0 having gated
/// nothing at all in most of the tree, and before per-language tables an
/// empty gate always died. The note is the only signal that the rest of
/// the tree went unchecked.
#[test]
fn a_language_only_manifest_warns_that_nothing_else_is_gated() {
    let dir = polyglot_tree(
        "paths = [\"branchy.c\", \"branchy.rs\"]\n\
         [thresholds.lang.c]\n\
         cyclomatic = 100\n",
    );

    let assert = cli(dir.path()).arg("check").assert().success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    assert!(
        stderr.contains("no global [thresholds] table: only c is gated"),
        "the run must say the rest of the tree is ungated: {stderr}"
    );
    assert!(
        offenders(&stderr).is_empty(),
        "nothing breaches a limit of 100: {stderr}"
    );
}
