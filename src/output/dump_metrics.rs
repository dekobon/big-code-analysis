// bca: suppress-file(halstead, nargs, nexits, nom)
// Terminal colored-tree writer: the offending sums (halstead.effort, nargs,
// nexits) are many-function aggregation artifacts, and the per-function
// nexits come from `?`-on-every-colored-write error propagation, not logic
// complexity — the same archetype as the sibling output writers (formats.rs,
// check_format.rs, dispatch.rs) which carry the identical marker.

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

/// Dump the `FuncSpace` metric tree with an explicit work stack rather
/// than recursion, so a pathologically deep space nesting (closures
/// within closures) cannot overflow the thread stack at dump time — an
/// uncatchable abort, forbidden by the no-panic rule (#700). Traversal
/// order and per-node glyphs are byte-identical to the prior recursive
/// form.
fn dump_space(
    space: &FuncSpace,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let mut stack: Vec<(&FuncSpace, String, bool)> = vec![(space, prefix.to_owned(), last)];

    while let Some((space, prefix, last)) = stack.pop() {
        let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}{pref}")?;

        intense_color(stdout, Color::Yellow)?;
        write!(stdout, "{}: ", space.kind)?;

        intense_color(stdout, Color::Cyan)?;
        write!(stdout, "{}", space.name.as_ref().map_or("", |name| name))?;

        intense_color(stdout, Color::Red)?;
        writeln!(stdout, " (@{})", space.start_line)?;

        let child_prefix = format!("{prefix}{pref_child}");
        dump_metrics(
            &space.metrics,
            &child_prefix,
            space.spaces.is_empty(),
            stdout,
        )?;

        // Push children in reverse so `pop()` visits them in source
        // order; the final child carries `last = true` for the closing
        // `` `- `` glyph, matching the recursive `split_last` form.
        let count = space.spaces.len();
        for (i, child) in space.spaces.iter().enumerate().rev() {
            stack.push((child, child_prefix.clone(), i + 1 == count));
        }
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
    // an integer. This *diverges* from the JSON serializer, which keeps
    // the `.0` (serde_json renders `2.0` as `"2.0"`, not `"2"`); the dump
    // drops it deliberately for terminal legibility, the same presentation
    // tradeoff the module header documents for rounded floats (#674).
    if float.fract() == 0.0 && float.abs() < F64_SAFE_INT_BOUND {
        #[allow(clippy::cast_possible_truncation)]
        return (float as i64).to_string();
    }
    format!("{float:.TEXT_FLOAT_DECIMALS$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_set::Metric;
    use crate::{LANG, MetricsOptions, Source, analyze};

    fn render(space: &FuncSpace) -> String {
        let mut buf = termcolor::NoColor::new(Vec::new());
        dump_space(space, "", true, &mut buf).expect("dump to in-memory buffer");
        String::from_utf8(buf.into_inner()).expect("utf-8 dump")
    }

    #[test]
    fn selection_mask_omits_unselected_metric_groups() {
        // `with_only(&[Loc])` must restrict the dump to the loc group:
        // the wire-driven projection (#674) elides unselected metrics, so
        // they never appear in the walked JSON object. A pre-#674 dump
        // printed all groups with default/zero stats, contradicting the
        // serialized "present => selected" contract (#700).
        let space = analyze(
            Source::new(LANG::Cpp, b"int a = 42;"),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .expect("snippet has a top-level FuncSpace");
        let out = render(&space);
        assert!(out.contains("loc\n"), "loc group must be present:\n{out}");
        for omitted in ["cognitive", "cyclomatic", "halstead", "nom", "abc"] {
            assert!(
                !out.contains(&format!("{omitted}\n")),
                "unselected `{omitted}` group must be omitted:\n{out}"
            );
        }
    }

    #[test]
    fn last_emitted_metric_group_uses_closing_connector() {
        // The genuinely-last emitted metric group must carry the closing
        // `` `- `` glyph rather than a dangling `|-` (#700, already made
        // dynamic by the wire projection in #674). For a non-class C
        // dump, the wmc/npm/npa class-only groups are elided, so the last
        // group line under the root `metrics` subtree must end the
        // subtree with `` `- ``. We locate the final group connector
        // (a `<rail>`- ` or `<rail>|- ` line whose label is a top-level
        // group, i.e. indented directly under `metrics`) and assert it is
        // the closing form.
        let space = analyze(
            Source::new(LANG::Cpp, b"int a = 42;"),
            MetricsOptions::default(),
        )
        .expect("snippet has a top-level FuncSpace");
        let out = render(&space);
        // The root metrics subtree indents its groups under `   ` (root
        // is the last/only space). The last such group line must use
        // `` `- ``; if any group dangled, the final group line would read
        // `|- `.
        let group_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("   |- ") || l.starts_with("   `- "))
            .collect();
        let last_group = group_lines.last().expect("at least one metric group");
        assert!(
            last_group.starts_with("   `- "),
            "the last emitted metric group must use the closing connector, got: {last_group:?}\n{out}"
        );
    }

    #[test]
    fn deeply_nested_spaces_dump_without_stack_overflow() {
        // The space walk is iterative (#700): a deep chain of nested
        // function spaces must dump without overflowing the thread stack.
        // Run on a small-stack thread so a recursion regression fails
        // loudly rather than relying on the test-runner stack.
        use crate::spaces::SpaceKind;
        const DEPTH: usize = 8_000;
        let handle = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let leaf = || FuncSpace {
                    name: Some("f".to_string()),
                    start_line: 1,
                    end_line: 1,
                    kind: SpaceKind::Function,
                    spaces: Vec::new(),
                    metrics: CodeMetrics::default(),
                    suppressed: crate::SuppressionScope::default(),
                };
                let mut root = leaf();
                let mut cursor = &mut root;
                for _ in 0..DEPTH {
                    cursor.spaces.push(leaf());
                    cursor = cursor.spaces.last_mut().expect("just pushed");
                }
                let mut sink = termcolor::NoColor::new(Vec::new());
                let ok = dump_space(&root, "", true, &mut sink).is_ok();
                // Flatten the chain before it drops: `FuncSpace`'s derived
                // `Drop` recurses through `spaces`, so a deep tree would
                // overflow the small stack on teardown and mask the dump
                // result. Hoisting each level's children out turns the
                // drop into an iterative one.
                let mut node = root;
                while let Some(child) = node.spaces.pop() {
                    node = child;
                }
                ok
            })
            .expect("spawn dump thread");
        assert!(
            handle.join().expect("dump thread must not overflow"),
            "deep space nesting must dump successfully"
        );
    }

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
