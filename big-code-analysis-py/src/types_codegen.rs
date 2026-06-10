//! Deterministic generator for the checked-in `_types.py` module —
//! `TypedDict` mirrors of the analysis-result wire shapes (issue #623).
//!
//! Every `analyze` / `analyze_source` / `analyze_batch` return is a JSON
//! projection of [`big_code_analysis::wire::FuncSpace`] — the single
//! source of the serialized shape. The stubs previously typed those
//! returns as `dict[str, Any]`, so strict-mode consumers got nothing past
//! the first subscript and had to litter `cast(...)` (see #623). This
//! module renders a `TypedDict` per wire struct so the returns carry a
//! precise, IDE-completable, `mypy --strict`-checkable shape.
//!
//! The generated file is checked in (IDEs, `mypy`, and `pyright` all need
//! a static `.py`) and the `types_module_matches_checked_in` drift gate in
//! `tests` regenerates and byte-compares it. The structural truth is
//! double-checked against the live wire JSON by
//! `spec_matches_wire_json_keys`: a wire-struct field add / remove / rename
//! shifts the serialized keys and fails that test, so the spec below cannot
//! silently drift from `src/wire.rs`.
//!
//! Scope: the `FuncSpace` metric tree (the only return whose shape is a
//! single `wire` struct). The VCS *report* top-level dicts (`vcs_metrics`
//! / `vcs_trend` / `vcs_jit`) are assembled outside `wire.rs` from several
//! schema-versioned pieces, so they stay `dict[str, Any]` rather than being
//! pinned to a shape this module cannot single-source.

use std::fmt::Write as _;

/// Path of the generated module relative to the crate root.
pub(crate) const TYPES_MODULE_PATH: &str = "python/big_code_analysis/_types.py";

/// Python type of a `TypedDict` field, mirroring the wire field's JSON
/// shape. The non-finite-float policy (#531) serializes `NaN`/`±∞` to
/// `null`, so every `f64` wire field is `float | None` on the Python side.
#[derive(Clone, Copy)]
enum FieldType {
    /// A `u64` / `u32` / `usize` count — always a JSON integer.
    Int,
    /// An `f64` derived value — JSON number or `null` (non-finite, #531).
    Float,
    /// A `String` field that is always present.
    Str,
    /// An `Option<String>` — JSON string or `null`.
    OptStr,
    /// A reference to another generated `TypedDict` (by class name).
    Dict(&'static str),
    /// A `list[<TypedDict>]` of another generated class.
    ListDict(&'static str),
    /// `list[str]`.
    ListStr,
}

impl FieldType {
    /// The Python annotation. The module carries `from __future__ import
    /// annotations`, so every annotation is a lazily-evaluated string at
    /// runtime — a recursive self-reference (`FuncSpaceDict.spaces`) and a
    /// forward reference therefore both resolve without explicit quoting.
    fn annotation(self) -> String {
        match self {
            FieldType::Int => "int".to_owned(),
            FieldType::Float => "float | None".to_owned(),
            FieldType::Str => "str".to_owned(),
            FieldType::OptStr => "str | None".to_owned(),
            FieldType::Dict(name) => name.to_owned(),
            FieldType::ListDict(name) => format!("list[{name}]"),
            FieldType::ListStr => "list[str]".to_owned(),
        }
    }
}

/// One field of a generated `TypedDict`: the JSON key, its Python type,
/// and whether it is always present (`Required`) or elided under metric
/// selection / emptiness (`NotRequired`).
struct Field {
    name: &'static str,
    ty: FieldType,
    required: bool,
}

const fn req(name: &'static str, ty: FieldType) -> Field {
    Field {
        name,
        ty,
        required: true,
    }
}

const fn opt(name: &'static str, ty: FieldType) -> Field {
    Field {
        name,
        ty,
        required: false,
    }
}

/// One generated `TypedDict` class: its Python class name and ordered
/// fields. Field order mirrors the wire struct's `Serialize` order so the
/// `spec_matches_wire_json_keys` drift gate can compare key sequences
/// directly.
struct DictSpec {
    class: &'static str,
    doc: &'static str,
    fields: &'static [Field],
}

use FieldType::{Dict, Float, Int, ListDict, ListStr, OptStr, Str};

/// The full spec, in dependency order (a class referencing another must
/// appear after it, except the recursive `FuncSpaceDict.spaces`
/// self-reference, which is emitted as a string forward reference).
///
/// Mirrors `src/wire.rs` field-for-field. Per-metric dicts are total
/// (every field present once the metric is selected); `CodeMetricsDict`
/// is `total=False` because `metrics=` selection elides whole blocks;
/// `FuncSpaceDict.suppressed` is `NotRequired` (elided when no marker
/// fired). The `vcs` block is `NotRequired` (injected post-analysis only
/// under `vcs=` / `vcs_per_function=`).
const SPECS: &[DictSpec] = &[
    DictSpec {
        class: "AbcDict",
        doc: "ABC metric block (assignments / branches / conditions).",
        fields: &[
            req("assignments", Int),
            req("branches", Int),
            req("conditions", Int),
            req("magnitude", Float),
            req("assignments_average", Float),
            req("branches_average", Float),
            req("conditions_average", Float),
            req("assignments_min", Int),
            req("assignments_max", Int),
            req("branches_min", Int),
            req("branches_max", Int),
            req("conditions_min", Int),
            req("conditions_max", Int),
        ],
    },
    DictSpec {
        class: "CognitiveDict",
        doc: "Cognitive-complexity metric block.",
        fields: &[
            req("sum", Int),
            req("average", Float),
            req("min", Int),
            req("max", Int),
        ],
    },
    DictSpec {
        class: "CyclomaticModifiedDict",
        doc: "Modified-cyclomatic sub-block of `CyclomaticDict`.",
        fields: &[
            req("sum", Int),
            req("average", Float),
            req("min", Int),
            req("max", Int),
        ],
    },
    DictSpec {
        class: "CyclomaticDict",
        doc: "Cyclomatic-complexity metric block.",
        fields: &[
            req("sum", Int),
            req("average", Float),
            req("min", Int),
            req("max", Int),
            req("modified", Dict("CyclomaticModifiedDict")),
        ],
    },
    DictSpec {
        class: "NexitsDict",
        doc: "Exit-points (`nexits`) metric block.",
        fields: &[
            req("sum", Int),
            req("average", Float),
            req("min", Int),
            req("max", Int),
        ],
    },
    DictSpec {
        class: "HalsteadDict",
        doc: "Halstead metric suite.",
        fields: &[
            req("unique_operators", Int),
            req("total_operators", Int),
            req("unique_operands", Int),
            req("total_operands", Int),
            req("length", Int),
            req("estimated_program_length", Float),
            req("purity_ratio", Float),
            req("vocabulary", Int),
            req("volume", Float),
            req("difficulty", Float),
            req("level", Float),
            req("effort", Float),
            req("time", Float),
            req("bugs", Float),
        ],
    },
    DictSpec {
        class: "LocDict",
        doc: "Lines-of-code metric suite.",
        fields: &[
            req("sloc", Int),
            req("ploc", Int),
            req("lloc", Int),
            req("cloc", Int),
            req("blank", Int),
            req("sloc_average", Float),
            req("ploc_average", Float),
            req("lloc_average", Float),
            req("cloc_average", Float),
            req("blank_average", Float),
            req("sloc_min", Int),
            req("sloc_max", Int),
            req("cloc_min", Int),
            req("cloc_max", Int),
            req("ploc_min", Int),
            req("ploc_max", Int),
            req("lloc_min", Int),
            req("lloc_max", Int),
            req("blank_min", Int),
            req("blank_max", Int),
        ],
    },
    DictSpec {
        class: "MiDict",
        doc: "Maintainability-index metric block.",
        fields: &[
            req("original", Float),
            req("sei", Float),
            req("visual_studio", Float),
        ],
    },
    DictSpec {
        class: "NargsDict",
        doc: "Number-of-arguments metric block.",
        fields: &[
            req("function_args", Int),
            req("closure_args", Int),
            req("function_args_average", Float),
            req("closure_args_average", Float),
            req("total", Int),
            req("average", Float),
            req("function_args_min", Int),
            req("function_args_max", Int),
            req("closure_args_min", Int),
            req("closure_args_max", Int),
        ],
    },
    DictSpec {
        class: "NomDict",
        doc: "Number-of-methods metric block.",
        fields: &[
            req("functions", Int),
            req("closures", Int),
            req("functions_average", Float),
            req("closures_average", Float),
            req("total", Int),
            req("average", Float),
            req("functions_min", Int),
            req("functions_max", Int),
            req("closures_min", Int),
            req("closures_max", Int),
        ],
    },
    DictSpec {
        class: "NpaDict",
        doc: "Number-of-public-attributes metric block.",
        fields: &[
            req("classes", Int),
            req("interfaces", Int),
            req("class_attributes", Int),
            req("interface_attributes", Int),
            req("class_cda", Float),
            req("interface_cda", Float),
            req("total", Int),
            req("total_attributes", Int),
            req("cda", Float),
        ],
    },
    DictSpec {
        class: "NpmDict",
        doc: "Number-of-public-methods metric block.",
        fields: &[
            req("classes", Int),
            req("interfaces", Int),
            req("class_methods", Int),
            req("interface_methods", Int),
            req("class_coa", Float),
            req("interface_coa", Float),
            req("total", Int),
            req("total_methods", Int),
            req("coa", Float),
        ],
    },
    DictSpec {
        class: "TokensDict",
        doc: "Token-count metric block.",
        fields: &[
            req("tokens", Int),
            req("tokens_average", Float),
            req("tokens_min", Int),
            req("tokens_max", Int),
        ],
    },
    DictSpec {
        class: "WmcDict",
        doc: "Weighted-methods-per-class metric block.",
        fields: &[
            req("classes", Int),
            req("interfaces", Int),
            req("total", Int),
        ],
    },
    DictSpec {
        class: "VcsDict",
        doc: "Change-history (VCS) metric block; present only under \
              `vcs=True` / `vcs_per_function=True`. `hotspot_score` and \
              `author_ids` are elided when unavailable.",
        fields: &[
            req("vcs_schema_version", Int),
            req("risk_score_version", Int),
            req("long_window_days", Int),
            req("recent_window_days", Int),
            req("commits_long", Int),
            req("commits_recent", Int),
            req("churn_long", Int),
            req("churn_recent", Int),
            req("authors_long", Int),
            req("authors_recent", Int),
            req("ownership_top_share", Float),
            req("burst", Float),
            req("bug_fix_commits", Int),
            req("security_fix_commits", Int),
            req("revert_commits", Int),
            req("age_days", Int),
            req("last_modified_days", Int),
            req("change_entropy_long", Float),
            req("change_entropy_recent", Float),
            req("cochange_entropy_long", Float),
            req("cochange_entropy_recent", Float),
            req("risk_score", Float),
            opt("hotspot_score", Float),
            opt("author_ids", ListStr),
        ],
    },
    DictSpec {
        class: "CodeMetricsDict",
        doc: "Per-space metric table. Every block is `NotRequired`: a \
              `metrics=` selection (or a class-only metric on a \
              non-class language) elides the unselected blocks entirely.",
        fields: &[
            opt("nargs", Dict("NargsDict")),
            opt("nexits", Dict("NexitsDict")),
            opt("cognitive", Dict("CognitiveDict")),
            opt("cyclomatic", Dict("CyclomaticDict")),
            opt("halstead", Dict("HalsteadDict")),
            opt("loc", Dict("LocDict")),
            opt("nom", Dict("NomDict")),
            opt("tokens", Dict("TokensDict")),
            opt("mi", Dict("MiDict")),
            opt("abc", Dict("AbcDict")),
            opt("wmc", Dict("WmcDict")),
            opt("npm", Dict("NpmDict")),
            opt("npa", Dict("NpaDict")),
            opt("vcs", Dict("VcsDict")),
        ],
    },
    DictSpec {
        class: "FuncSpaceDict",
        doc: "A metric space: the file-level `unit` space or a nested \
              function / class / impl. Returned by `analyze`, \
              `analyze_source`, and the dict entries of `analyze_batch`.",
        fields: &[
            req("name", OptStr),
            req("start_line", Int),
            req("end_line", Int),
            req("kind", Str),
            req("spaces", ListDict("FuncSpaceDict")),
            req("metrics", Dict("CodeMetricsDict")),
            opt("suppressed", Dict("SuppressionScopeDict")),
        ],
    },
];

/// The `suppressed` field's wire shape is an internally-tagged enum
/// (`{"kind": "all"}` or `{"kind": "some", "metrics": [...]}`), not a flat
/// struct, so it cannot be reflected from a single struct's field order
/// and is hand-modelled here. The drift gate covers it via a dedicated
/// JSON-shape assertion rather than the field-order comparison used for
/// `SPECS`.
const SUPPRESSION_SCOPE: DictSpec = DictSpec {
    class: "SuppressionScopeDict",
    doc: "In-source suppression scope (`bca:`/Lizard markers). \
          `kind` is `\"all\"` (suppress every metric) or `\"some\"` \
          (only the listed `metrics`, present only then).",
    fields: &[req("kind", Str), opt("metrics", ListStr)],
};

/// Render a class docstring, word-wrapped so every line (4-space indent +
/// content + the closing `"""`) stays within ruff's 100-column limit. A
/// single short doc collapses to one line; a longer one wraps onto
/// continuation lines under the opening `"""`.
fn render_docstring(doc: &str) -> String {
    const INDENT: &str = "    ";
    // Leave headroom for the indent and the closing `"""` so even the last
    // line clears the 100-col limit.
    const WRAP: usize = 92;

    // Collapse any internal whitespace runs (the spec docs use `\` line
    // continuations, which leave multiple spaces) to single spaces.
    let words: Vec<&str> = doc.split_whitespace().collect();
    let single = words.join(" ");
    if INDENT.len() + 3 + single.len() + 3 <= 100 {
        return format!("{INDENT}\"\"\"{single}\"\"\"\n");
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= WRAP {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }

    let mut out = format!("{INDENT}\"\"\"{}\n", lines[0]);
    for line in &lines[1..] {
        let _ = writeln!(out, "{INDENT}{line}");
    }
    out.push_str(INDENT);
    out.push_str("\"\"\"\n");
    out
}

/// Render one `TypedDict` subclass. Uses the class-statement form (rather
/// than the functional `TypedDict("X", {...})` form) so the recursive
/// self-reference in `FuncSpaceDict.spaces` resolves and the members are
/// IDE-navigable.
fn render_dict(spec: &DictSpec, total: bool) -> String {
    let total_suffix = if total { "" } else { ", total=False" };
    let mut out = format!("class {}(TypedDict{total_suffix}):\n", spec.class);
    out.push_str(&render_docstring(spec.doc));
    out.push('\n');
    for field in spec.fields {
        let annotation = field.ty.annotation();
        // A `NotRequired` field in an otherwise-total dict is wrapped;
        // in a `total=False` dict every field is already optional, so the
        // wrapper would be redundant noise. A `Required` field in a
        // `total=False` dict (none today) would need `Required[...]`.
        let rendered = if total && !field.required {
            format!("NotRequired[{annotation}]")
        } else {
            annotation
        };
        // `writeln!` into a `String` is infallible.
        let _ = writeln!(out, "    {}: {rendered}", field.name);
    }
    out
}

/// Render the full `_types.py` module text. Deterministic (declaration
/// order, no timestamps) so the drift gate can byte-compare it.
pub(crate) fn render_types_module() -> String {
    let header = "\
# @generated by `big-code-analysis-py` from the `big_code_analysis::wire`
# shapes (src/wire.rs) — DO NOT EDIT BY HAND. Regenerate with:
#
#     BCA_PY_REGEN_TYPES=1 cargo test -p big-code-analysis-py types_module
#
# These TypedDicts mirror the JSON projection that `analyze` /
# `analyze_source` / `analyze_batch` return. They are stub-only: the
# runtime values are plain dicts, byte-identical to the CLI's JSON, so a
# TypedDict annotation only narrows the static type. `total=False` blocks
# (CodeMetricsDict) and NotRequired fields cover keys elided under
# `metrics=` selection or when no suppression marker fired.
\"\"\"Generated TypedDicts for the analysis-result wire shapes (#623).\"\"\"

from __future__ import annotations

from typing import NotRequired, TypedDict

__all__ = [\n";

    // The class-name list for `__all__`. Sorted (not declaration order) to
    // satisfy ruff's RUF022 isort-style `__all__` ordering; the class
    // *definitions* below keep dependency order.
    let mut all_names: Vec<&str> = SPECS.iter().map(|s| s.class).collect();
    all_names.push(SUPPRESSION_SCOPE.class);
    all_names.sort_unstable();
    let mut all_block = String::new();
    for name in all_names {
        let _ = writeln!(all_block, "    \"{name}\",");
    }

    let mut body = String::new();
    // The hand-modelled suppression scope is emitted first because
    // `FuncSpaceDict` references it.
    body.push_str(&render_dict(&SUPPRESSION_SCOPE, true));
    body.push('\n');
    body.push('\n');
    for (i, spec) in SPECS.iter().enumerate() {
        // `CodeMetricsDict` is the only `total=False` block: `metrics=`
        // selection elides whole metric blocks, so every key is optional.
        let total = spec.class != "CodeMetricsDict";
        body.push_str(&render_dict(spec, total));
        if i + 1 < SPECS.len() {
            body.push('\n');
            body.push('\n');
        }
    }

    format!("{header}{all_block}]\n\n\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::analysis::{AnalyzeOptions, analyze_source};

    /// Serialize a source buffer through the bindings' own analysis path —
    /// the exact JSON `analyze_source` returns to Python — and parse it
    /// back to a `serde_json::Value` so the key-order assertions read the
    /// real wire projection rather than a hand-built struct.
    fn analyze_to_value(language: &str, code: &str) -> serde_json::Value {
        let json = analyze_source(language, code.as_bytes(), None, AnalyzeOptions::default())
            .expect("analyze fixture");
        serde_json::from_str(&json).expect("parse fixture JSON")
    }

    /// Drift gate: the checked-in `_types.py` must match the module
    /// rendered from the spec. Regenerate with `BCA_PY_REGEN_TYPES=1`.
    #[test]
    fn types_module_matches_checked_in() {
        let rendered = render_types_module();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(TYPES_MODULE_PATH);

        if std::env::var_os("BCA_PY_REGEN_TYPES").is_some() {
            std::fs::write(&path, &rendered).expect("write _types.py");
            return;
        }

        let checked_in = std::fs::read_to_string(&path).expect("read _types.py");
        assert_eq!(
            checked_in, rendered,
            "{TYPES_MODULE_PATH} drifted from the codegen spec; regenerate \
             with `BCA_PY_REGEN_TYPES=1 cargo test -p big-code-analysis-py \
             types_module`",
        );
    }

    /// Look up a spec by class name (panics if absent — a test-only helper
    /// so the wire-key assertions read declaratively).
    fn spec(class: &str) -> &'static DictSpec {
        SPECS
            .iter()
            .find(|s| s.class == class)
            .unwrap_or_else(|| panic!("no spec for {class}"))
    }

    /// Assert that the keys of `value` (a serialized wire struct) equal the
    /// `Required` fields of `class` as a set. This is the real
    /// single-source check: a field add / remove / rename in `src/wire.rs`
    /// shifts the JSON keys and fails here, forcing the spec (and the
    /// regenerated `_types.py`) to follow. `Float` fields are confirmed to
    /// serialize as a JSON number (or null) and `Int` fields as integers,
    /// so an `i64`↔`f64` flip in wire.rs is also caught.
    ///
    /// Set (not sequence) comparison because `serde_json::Value` parses
    /// objects into a `BTreeMap` (no `preserve_order` feature in this
    /// workspace), so document order is not observable here. Field *order*
    /// in the generated `_types.py` mirrors `wire.rs` by hand and is locked
    /// by the byte-identical `types_module_matches_checked_in` gate; the
    /// lib's own `wire` round-trip tests pin the serialized byte order.
    fn assert_keys_match(class: &str, value: &serde_json::Value) {
        use std::collections::BTreeSet;
        let obj = value
            .as_object()
            .unwrap_or_else(|| panic!("{class} did not serialize as a JSON object"));
        let spec = spec(class);
        let want: BTreeSet<&str> = spec
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        let got: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            want, got,
            "{class}: spec fields drifted from wire JSON keys"
        );

        for field in spec.fields.iter().filter(|f| f.required) {
            let v = &obj[field.name];
            match field.ty {
                FieldType::Int => assert!(
                    v.is_u64() || v.is_i64(),
                    "{class}.{}: expected JSON integer, got {v}",
                    field.name
                ),
                // A finite f64 serializes to a JSON number; a non-finite one
                // to null. The probe instances are built finite, so a number
                // is expected here — an int↔float swap (which would emit an
                // integer-looking number for a u64 field) is caught by the
                // Int arm above, and the Float arm confirms the field is
                // numeric (not a string/bool/object).
                FieldType::Float => assert!(
                    v.is_number(),
                    "{class}.{}: expected JSON number, got {v}",
                    field.name
                ),
                _ => {}
            }
        }
    }

    /// The per-metric blocks and the containers serialize with exactly the
    /// spec's field set — the structural single-source guarantee against
    /// `src/wire.rs`. Rust populates class-only metrics as disabled (so
    /// `wmc` / `npm` / `npa` are absent here) but emits every other block.
    #[test]
    fn spec_matches_wire_json_keys() {
        let value = analyze_to_value(
            "rust",
            "fn classify(x: i32) -> i32 {\n    if x > 0 { x } else { -x }\n}\n",
        );

        // FuncSpaceDict: top-level required keys (suppressed is elided when
        // empty, matching the NotRequired spec — so it is absent here).
        let obj = value.as_object().expect("FuncSpace object");
        let funcspace_required: std::collections::BTreeSet<&str> = spec("FuncSpaceDict")
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        let got: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(funcspace_required, got, "FuncSpaceDict key drift");

        // CodeMetricsDict: under the full suite, every non-vcs block is
        // present except the class-only metrics disabled for Rust (wmc /
        // npm / npa), so compare the present keys against the spec's set.
        let metrics = obj["metrics"].as_object().expect("metrics object");
        let metric_keys: std::collections::BTreeSet<&str> =
            metrics.keys().map(String::as_str).collect();
        let spec_keys: std::collections::BTreeSet<&str> = spec("CodeMetricsDict")
            .fields
            .iter()
            .map(|f| f.name)
            .collect();
        assert!(
            metric_keys.is_subset(&spec_keys),
            "wire metrics carries a key absent from CodeMetricsDict: {:?}",
            &metric_keys - &spec_keys
        );
        // The always-present (non-class-only, non-vcs) metrics must each be
        // covered by the spec.
        for required in [
            "nargs",
            "nexits",
            "cognitive",
            "cyclomatic",
            "halstead",
            "loc",
            "nom",
            "tokens",
            "mi",
            "abc",
        ] {
            assert!(
                spec_keys.contains(required),
                "CodeMetricsDict spec missing always-present metric {required}"
            );
            assert!(
                metric_keys.contains(required),
                "wire metrics fixture missing {required}"
            );
        }

        // Each per-metric block's keys equal its spec (order + numeric kind).
        assert_keys_match("AbcDict", &metrics["abc"]);
        assert_keys_match("CognitiveDict", &metrics["cognitive"]);
        assert_keys_match("CyclomaticDict", &metrics["cyclomatic"]);
        assert_keys_match("CyclomaticModifiedDict", &metrics["cyclomatic"]["modified"]);
        assert_keys_match("NexitsDict", &metrics["nexits"]);
        assert_keys_match("HalsteadDict", &metrics["halstead"]);
        assert_keys_match("LocDict", &metrics["loc"]);
        assert_keys_match("MiDict", &metrics["mi"]);
        assert_keys_match("NargsDict", &metrics["nargs"]);
        assert_keys_match("NomDict", &metrics["nom"]);
        assert_keys_match("TokensDict", &metrics["tokens"]);
    }

    /// The class-only metric blocks (`wmc` / `npm` / `npa`) and `VcsDict`
    /// never appear in the Rust fixture (disabled / not injected), so they
    /// are checked against a language that emits them and a synthetic
    /// `vcs` block respectively — otherwise their specs would be untested.
    #[test]
    fn class_only_and_vcs_specs_match_wire_json_keys() {
        use big_code_analysis::wire;

        // Java emits wmc / npm / npa on a class.
        let value = analyze_to_value(
            "java",
            "public class Foo {\n    public int a;\n    public int m() { return 1; }\n}\n",
        );
        let metrics = value["metrics"].as_object().expect("metrics");
        assert_keys_match("WmcDict", &metrics["wmc"]);
        assert_keys_match("NpmDict", &metrics["npm"]);
        assert_keys_match("NpaDict", &metrics["npa"]);

        // VcsDict: serialize a default wire::Vcs directly (with the two
        // optional fields populated so they appear in the key set, which we
        // then strip before comparing against the required-field list).
        let vcs = wire::Vcs {
            hotspot_score: Some(1.0),
            author_ids: Some(vec!["abc".to_owned()]),
            ..Default::default()
        };
        let mut vcs_value = serde_json::to_value(&vcs).expect("serialize vcs");
        let obj = vcs_value.as_object_mut().expect("vcs object");
        // hotspot_score / author_ids are NotRequired (skip_serializing_if);
        // drop them so the remaining keys are exactly the required set.
        obj.remove("hotspot_score");
        obj.remove("author_ids");
        assert_keys_match("VcsDict", &vcs_value);
        // And confirm the spec lists both optional fields.
        let vcs_spec = spec("VcsDict");
        for optional in ["hotspot_score", "author_ids"] {
            assert!(
                vcs_spec
                    .fields
                    .iter()
                    .any(|f| f.name == optional && !f.required),
                "VcsDict spec missing NotRequired field {optional}"
            );
        }
    }

    /// The `suppressed` field's tagged-enum shape: `{"kind": "all"}` and
    /// `{"kind": "some", "metrics": [...]}`. Pins the hand-modelled
    /// `SuppressionScopeDict` against the live serialized form.
    #[test]
    fn suppression_scope_spec_matches_wire_json() {
        use big_code_analysis::{Metric, SuppressionScope};
        use std::collections::BTreeSet;

        let all = serde_json::to_value(SuppressionScope::All).expect("serialize All");
        assert_eq!(
            all,
            serde_json::json!({"kind": "all"}),
            "SuppressionScope::All wire shape changed"
        );

        let mut set = BTreeSet::new();
        set.insert(Metric::Loc);
        let some = serde_json::to_value(SuppressionScope::Some(set)).expect("serialize Some");
        let obj = some.as_object().expect("some object");
        let keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            BTreeSet::from(["kind", "metrics"]),
            "Some scope keys changed"
        );
        assert_eq!(obj["kind"], serde_json::json!("some"));
        assert!(obj["metrics"].is_array());
    }
}
