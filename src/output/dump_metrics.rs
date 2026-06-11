// bca: suppress-file(halstead, nargs, nexits)
// Terminal per-metric dump serializer; the offenders are mechanical-writer
// aggregation artifacts, not per-function logic complexity.

// `dump_value` takes `f64`; integral `u64` metric accessors are widened with
// `as f64` at the call sites (#530). Each cast is bounded by its count.
#![allow(clippy::cast_precision_loss)]

use termcolor::{Color, StandardStream, WriteColor};

use crate::abc;
use crate::cognitive;
use crate::cyclomatic;
use crate::halstead;
use crate::loc;
use crate::mi;
use crate::nargs;
use crate::nexits;
use crate::nom;
use crate::npa;
use crate::npm;
use crate::tokens;
use crate::wmc;

use crate::output::ColorMode;
use crate::spaces::{CodeMetrics, FuncSpace};

use crate::tools::{color, intense_color};

/// Dumps the metrics of a code.
///
/// Returns a [`Result`] value, when an error occurs.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] produced by the color-aware
/// writer that backs `stdout` (broken pipe, write failure, …).
///
/// # Examples
///
/// ```
/// use big_code_analysis::{analyze, dump_root, LANG, MetricsOptions, Source};
///
/// // Compute metrics via the non-generic `analyze` entry point.
/// let space = analyze(
///     Source::new(LANG::Cpp, b"int a = 42;"),
///     MetricsOptions::default(),
/// )
/// .expect("snippet has a top-level FuncSpace");
///
/// // Dump all metrics
/// dump_root(&space).unwrap();
/// ```
///
/// [`Result`]: #variant.Result
pub fn dump_root(space: &FuncSpace) -> std::io::Result<()> {
    dump_root_with_color(space, ColorMode::Always)
}

/// Like [`dump_root`], but the caller selects the [`ColorMode`].
///
/// `bca` resolves a `--color` flag, the `NO_COLOR` convention, and
/// stdout tty detection into a mode and passes it here so piped output
/// is escape-free by default. The bare [`dump_root`] keeps the
/// historical always-colored behavior for backward compatibility.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] produced by the color-aware
/// writer that backs `stdout` (broken pipe, write failure, …).
pub fn dump_root_with_color(space: &FuncSpace, color_mode: ColorMode) -> std::io::Result<()> {
    let stdout = StandardStream::stdout(color_mode.to_color_choice());
    let mut stdout = stdout.lock();
    dump_space(space, "", true, &mut stdout)?;
    color(&mut stdout, Color::White)?;

    Ok(())
}

fn dump_space(
    space: &FuncSpace,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Yellow)?;
    write!(stdout, "{}: ", space.kind)?;

    intense_color(stdout, Color::Cyan)?;
    write!(stdout, "{}", space.name.as_ref().map_or("", |name| name))?;

    intense_color(stdout, Color::Red)?;
    writeln!(stdout, " (@{})", space.start_line)?;

    let prefix = format!("{prefix}{pref_child}");
    dump_metrics(&space.metrics, &prefix, space.spaces.is_empty(), stdout)?;

    if let Some((last, spaces)) = space.spaces.split_last() {
        for space in spaces {
            dump_space(space, &prefix, false, stdout)?;
        }
        dump_space(last, &prefix, true, stdout)?;
    }

    Ok(())
}

fn dump_metrics(
    metrics: &CodeMetrics,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Yellow)?;
    writeln!(stdout, "metrics")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_cognitive(&metrics.cognitive, &prefix, false, stdout)?;
    dump_cyclomatic(&metrics.cyclomatic, &prefix, false, stdout)?;
    dump_nargs(&metrics.nargs, &prefix, false, stdout)?;
    dump_nexits(&metrics.nexits, &prefix, false, stdout)?;
    dump_halstead(&metrics.halstead, &prefix, false, stdout)?;
    dump_loc(&metrics.loc, &prefix, false, stdout)?;
    dump_nom(&metrics.nom, &prefix, false, stdout)?;
    dump_tokens(&metrics.tokens, &prefix, false, stdout)?;
    dump_mi(&metrics.mi, &prefix, false, stdout)?;
    dump_abc(&metrics.abc, &prefix, false, stdout)?;
    dump_wmc(&metrics.wmc, &prefix, false, stdout)?;
    dump_npm(&metrics.npm, &prefix, false, stdout)?;
    dump_npa(&metrics.npa, &prefix, true, stdout)
}

fn dump_cognitive(
    stats: &cognitive::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "cognitive")?;

    let prefix = format!("{prefix}{pref_child}");

    dump_value("sum", stats.cognitive_sum() as f64, &prefix, false, stdout)?;
    dump_value("average", stats.cognitive_average(), &prefix, true, stdout)
}

fn dump_cyclomatic(
    stats: &cyclomatic::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "cyclomatic")?;

    let prefix = format!("{prefix}{pref_child}");

    dump_value("sum", stats.cyclomatic_sum() as f64, &prefix, false, stdout)?;
    dump_value("average", stats.cyclomatic_average(), &prefix, true, stdout)
}

fn dump_halstead(
    stats: &halstead::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "halstead")?;

    let prefix = format!("{prefix}{pref_child}");

    dump_value(
        "unique_operators",
        stats.unique_operators() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "total_operators",
        stats.total_operators() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "unique_operands",
        stats.unique_operands() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "total_operands",
        stats.total_operands() as f64,
        &prefix,
        false,
        stdout,
    )?;

    dump_value("length", stats.length() as f64, &prefix, false, stdout)?;
    dump_value(
        "estimated_program_length",
        stats.estimated_program_length(),
        &prefix,
        false,
        stdout,
    )?;
    dump_value("purity_ratio", stats.purity_ratio(), &prefix, false, stdout)?;
    dump_value(
        "vocabulary",
        stats.vocabulary() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("volume", stats.volume(), &prefix, false, stdout)?;
    dump_value("difficulty", stats.difficulty(), &prefix, false, stdout)?;
    dump_value("level", stats.level(), &prefix, false, stdout)?;
    dump_value("effort", stats.effort(), &prefix, false, stdout)?;
    dump_value("time", stats.time(), &prefix, false, stdout)?;
    dump_value("bugs", stats.bugs(), &prefix, true, stdout)
}

fn dump_loc(
    stats: &loc::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "loc")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value("sloc", stats.sloc() as f64, &prefix, false, stdout)?;
    dump_value("ploc", stats.ploc() as f64, &prefix, false, stdout)?;
    dump_value("lloc", stats.lloc() as f64, &prefix, false, stdout)?;
    dump_value("cloc", stats.cloc() as f64, &prefix, false, stdout)?;
    dump_value("blank", stats.blank() as f64, &prefix, true, stdout)
}

fn dump_nom(
    stats: &nom::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "nom")?;

    let prefix = format!("{prefix}{pref_child}");
    // Use the subtree-aggregate counts (`*_sum`), matching the JSON
    // serializer and `Display`. `functions()`/`closures()` are this
    // space's *immediate* counts, which would not sum to the aggregate
    // `total()` at any parent space (e.g. a Rust file whose functions all
    // live inside `impl`/`mod` would print `functions: 0, total: N`).
    dump_value(
        "functions",
        stats.functions_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "closures",
        stats.closures_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("total", stats.total() as f64, &prefix, true, stdout)
}

fn dump_tokens(
    stats: &tokens::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "tokens")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value("sum", stats.tokens_sum() as f64, &prefix, false, stdout)?;
    dump_value("average", stats.tokens_average(), &prefix, false, stdout)?;
    dump_value("min", stats.tokens_min() as f64, &prefix, false, stdout)?;
    dump_value("max", stats.tokens_max() as f64, &prefix, true, stdout)
}

fn dump_mi(
    stats: &mi::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "mi")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value("original", stats.original(), &prefix, false, stdout)?;
    dump_value("sei", stats.sei(), &prefix, false, stdout)?;
    dump_value(
        "visual_studio",
        stats.visual_studio(),
        &prefix,
        true,
        stdout,
    )
}

fn dump_nargs(
    stats: &nargs::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "nargs")?;

    let prefix = format!("{prefix}{pref_child}");
    // Subtree-aggregate counts (`*_sum`), matching the JSON serializer:
    // `total`/`average` are already aggregates, so the per-space
    // `function_args()`/`closure_args()` would not sum to `total` at a parent.
    dump_value(
        "function_args",
        stats.function_args_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "closure_args",
        stats.closure_args_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("total", stats.total() as f64, &prefix, false, stdout)?;
    dump_value("average", stats.average(), &prefix, true, stdout)
}

fn dump_nexits(
    stats: &nexits::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let pref = if last { "`- " } else { "|- " };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    write!(stdout, "nexits: ")?;

    color(stdout, Color::White)?;
    writeln!(stdout, "{}", stats.nexits())
}

fn dump_abc(
    stats: &abc::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "abc")?;

    let prefix = format!("{prefix}{pref_child}");

    dump_value(
        "assignments",
        stats.assignments_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "branches",
        stats.branches_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "conditions",
        stats.conditions_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("magnitude", stats.magnitude_sum(), &prefix, true, stdout)
}

fn dump_wmc(
    stats: &wmc::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    if stats.is_disabled() {
        return Ok(());
    }

    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "wmc")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value(
        "classes",
        stats.class_wmc_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "interfaces",
        stats.interface_wmc_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("total", stats.total_wmc() as f64, &prefix, true, stdout)
}

fn dump_npm(
    stats: &npm::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    if stats.is_disabled() {
        return Ok(());
    }

    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "npm")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value(
        "classes",
        stats.class_npm_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "interfaces",
        stats.interface_npm_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("total", stats.total_npm() as f64, &prefix, false, stdout)?;
    dump_value("coa", stats.total_coa(), &prefix, true, stdout)
}

fn dump_npa(
    stats: &npa::Stats,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    if stats.is_disabled() {
        return Ok(());
    }

    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "npa")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_value(
        "classes",
        stats.class_npa_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value(
        "interfaces",
        stats.interface_npa_sum() as f64,
        &prefix,
        false,
        stdout,
    )?;
    dump_value("total", stats.total_npa() as f64, &prefix, false, stdout)?;
    dump_value("cda", stats.total_cda(), &prefix, true, stdout)
}

fn dump_value(
    name: &str,
    val: f64,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let pref = if last { "`- " } else { "|- " };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Magenta)?;
    write!(stdout, "{name}: ")?;

    color(stdout, Color::White)?;
    writeln!(stdout, "{val}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LANG, MetricsOptions, Source, analyze};

    /// Value printed after `{field}:` in the FIRST `{block}` metric block of
    /// the dump — i.e. the root `Unit`'s, which is emitted before any child
    /// space. `{val}` Display renders whole f64s without a decimal point, so
    /// callers can compare against `"0"`.
    fn root_block_field(out: &str, block: &str, field: &str) -> String {
        let body = &out[out
            .find(&format!("{block}\n"))
            .expect("metric block present")..];
        let at = body.find(&format!("{field}: ")).expect("field present") + field.len() + 2;
        body[at..].lines().next().unwrap_or("").trim().to_owned()
    }

    /// Regression for the parent-space aggregate bug: `dump_nom` / `dump_nargs`
    /// must print the SUBTREE-AGGREGATE counts (`functions_sum` / `fn_args_sum`,
    /// matching the JSON serializer and `Display`), not the space's IMMEDIATE
    /// counts — which are 0 at any parent whose functions all live in a nested
    /// module/impl, and would not sum to the aggregate `total`.
    #[test]
    fn dump_nom_and_nargs_use_subtree_aggregates_at_parent_space() {
        // The one function (with args) is nested in `mod m`, so the root Unit's
        // immediate function/arg counts are 0 while the subtree aggregates are
        // not — the exact shape that exposed the bug.
        let space = analyze(
            Source::new(
                LANG::Rust,
                b"mod m { fn a(x: i32, y: i32) -> i32 { x + y } }",
            ),
            MetricsOptions::default(),
        )
        .expect("snippet has a top-level FuncSpace");

        let mut buf = termcolor::NoColor::new(Vec::new());
        dump_space(&space, "", true, &mut buf).expect("dump to in-memory buffer");
        let out = String::from_utf8(buf.into_inner()).expect("utf-8 dump");

        assert_ne!(
            root_block_field(&out, "nom", "functions"),
            "0",
            "root nom must print functions_sum (aggregate), not the immediate 0:\n{out}"
        );
        assert_ne!(
            root_block_field(&out, "nargs", "functions"),
            "0",
            "root nargs must print fn_args_sum (aggregate), not the immediate 0:\n{out}"
        );
    }

    /// Regression for #562: the two Halstead dump labels must use the
    /// underscore key that matches the JSON/CSV key name, so a user can grep
    /// the same token across `dump` and JSON. The space-separated forms
    /// (`estimated program length` / `purity ratio`) were the only outliers.
    #[test]
    fn dump_halstead_labels_use_underscore_keys() {
        let space = analyze(
            Source::new(LANG::Cpp, b"int a = 42;"),
            MetricsOptions::default(),
        )
        .expect("snippet has a top-level FuncSpace");

        let mut buf = termcolor::NoColor::new(Vec::new());
        dump_space(&space, "", true, &mut buf).expect("dump to in-memory buffer");
        let out = String::from_utf8(buf.into_inner()).expect("utf-8 dump");

        assert!(
            out.contains("estimated_program_length: "),
            "dump must use the underscore key `estimated_program_length`:\n{out}"
        );
        assert!(
            out.contains("purity_ratio: "),
            "dump must use the underscore key `purity_ratio`:\n{out}"
        );
        assert!(
            !out.contains("estimated program length"),
            "dump must not emit the space-separated `estimated program length`:\n{out}"
        );
        assert!(
            !out.contains("purity ratio"),
            "dump must not emit the space-separated `purity ratio`:\n{out}"
        );
    }
}
