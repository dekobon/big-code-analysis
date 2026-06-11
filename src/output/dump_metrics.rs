//! Terminal per-metric dump serializer.
//!
//! The dump tree is driven by [`wire::CodeMetrics`] — the serialized
//! metric shape — rather than hand-picking a per-metric subset of stats
//! (issue #674). Projecting the compute metrics through the wire form and
//! walking the resulting JSON object guarantees the dump's field set is
//! *uniform by construction*: every leaf the JSON output carries appears
//! here under the same key, and a new metric field shows up automatically
//! with no edit to this file.
//!
//! Field *order* in this text view is serde_json's default sorted-key order
//! (`Value::Object` is a `BTreeMap` because `preserve_order` is deliberately
//! not enabled — see the root `Cargo.toml`), which is deterministic and
//! differs from the JSON serializer's struct-field order. The uniform field
//! *set* is what #674 requires; matching JSON's order would mean enabling
//! `preserve_order` workspace-wide, which would perturb the frozen
//! code-climate / SARIF fingerprint contracts (#559).
//!
//! The other deliberate divergence from JSON is presentation: float values
//! render rounded to [`TEXT_FLOAT_DECIMALS`] decimals in this text view,
//! whereas JSON keeps full precision. Non-finite floats (which serialize
//! to JSON `null`) render as `NaN`, matching the prior dump and the
//! human-readable `numfmt` arm.

use termcolor::{Color, StandardStream, WriteColor};

use serde_json::Value;

use crate::output::ColorMode;
use crate::output::numfmt::F64_SAFE_INT_BOUND;
use crate::spaces::{CodeMetrics, FuncSpace};
use crate::wire;

use crate::tools::{color, intense_color};

/// Decimal places used when rendering a non-integer float in the text
/// dump. JSON output keeps full precision; the terminal view trades the
/// trailing noise for legibility (issue #674).
const TEXT_FLOAT_DECIMALS: usize = 2;

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

    // Project the compute metrics through the wire shape and walk the
    // serialized object so the dump's field set is the JSON field set
    // exactly (issue #674). Disabled class-only metrics (`wmc`/`npm`/`npa`
    // on a non-class language) are already elided by the `From` impl, so
    // they never appear in the object and need no per-metric guard here.
    let wire_metrics = wire::CodeMetrics::from(metrics);
    let Value::Object(groups) = serde_json::to_value(&wire_metrics).unwrap_or(Value::Null) else {
        return Ok(());
    };

    let prefix = format!("{prefix}{pref_child}");
    let last_index = groups.len().saturating_sub(1);
    for (index, (name, value)) in groups.iter().enumerate() {
        dump_group(name, value, &prefix, index == last_index, stdout)?;
    }
    Ok(())
}

/// Render one metric group (`cognitive`, `loc`, …) as a green-labelled
/// subtree, then walk its leaves. A nested object leaf (e.g.
/// `cyclomatic.modified`) recurses as its own subtree, so the rendered
/// shape always mirrors the JSON nesting.
fn dump_group(
    name: &str,
    value: &Value,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "{name}")?;

    let prefix = format!("{prefix}{pref_child}");
    dump_object(value, &prefix, stdout)
}

/// Walk the leaves of a metric object, emitting one `name: value` line
/// per scalar and recursing into any nested object (rendered as a green
/// subtree, matching the JSON nesting). A non-object value is ignored
/// (the wire metric groups are always objects).
fn dump_object(value: &Value, prefix: &str, stdout: &mut dyn WriteColor) -> std::io::Result<()> {
    let Value::Object(fields) = value else {
        return Ok(());
    };
    let last_index = fields.len().saturating_sub(1);
    for (index, (name, leaf)) in fields.iter().enumerate() {
        let last = index == last_index;
        if leaf.is_object() {
            dump_group(name, leaf, prefix, last, stdout)?;
        } else {
            dump_value(name, leaf, prefix, last, stdout)?;
        }
    }
    Ok(())
}

/// Emit a single `name: value` leaf. Floats render rounded to
/// [`TEXT_FLOAT_DECIMALS`] decimals (text view only — JSON keeps full
/// precision); integers print verbatim; a JSON `null` (a non-finite
/// metric) renders as `NaN`, matching the prior dump.
fn dump_value(
    name: &str,
    value: &Value,
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
    writeln!(stdout, "{}", format_leaf(value))
}

/// Format a scalar wire leaf for the text view. Integral numbers print
/// without a decimal point; non-integral floats round to
/// [`TEXT_FLOAT_DECIMALS`] places; `null` (a non-finite metric) becomes
/// `NaN`.
fn format_leaf(value: &Value) -> String {
    match value {
        Value::Null => "NaN".to_owned(),
        Value::Number(n) => format_number(n),
        // The wire metric leaves are only numbers or null; render anything
        // else verbatim rather than panicking on an unexpected shape.
        other => other.to_string(),
    }
}

/// Render a JSON number: an integer prints without a decimal point; a
/// float rounds to [`TEXT_FLOAT_DECIMALS`] places, after which a trailing
/// `.00` is dropped so a whole-valued average reads like a count.
fn format_number(n: &serde_json::Number) -> String {
    if let Some(int) = n.as_u64() {
        return int.to_string();
    }
    if let Some(int) = n.as_i64() {
        return int.to_string();
    }
    let Some(float) = n.as_f64() else {
        return n.to_string();
    };
    // A safe-integer-valued float (e.g. an exact `2.0` average) prints as
    // an integer, matching the JSON serializer's trailing-`.0` elision.
    if float.fract() == 0.0 && float.abs() < F64_SAFE_INT_BOUND {
        #[allow(clippy::cast_possible_truncation)]
        return (float as i64).to_string();
    }
    format!("{float:.TEXT_FLOAT_DECIMALS$}")
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
