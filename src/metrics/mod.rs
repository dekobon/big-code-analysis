// bca: suppress-file(halstead)
// This module's production content is two tiny helpers (`average` and the
// `NonFinite` serialize wrapper); its bulk is the `#[cfg(test)]`
// cross-format serialization suite (#531), which is operator/operand-dense
// by nature (serde_json / serde_yaml / toml / serde_cbor round-trips over
// every non-finite case). The file-level `halstead.effort` aggregate is
// therefore a test-artifact, not production logic complexity — cognitive /
// cyclomatic stay enforced.

//! Per-metric implementations.
//!
//! Each submodule defines one maintainability metric, its per-language
//! traits, and its `Stats` accumulator. See the crate-level docs for an
//! overview of the metric suite.

/// Assignment / Branch / Condition counts.
pub mod abc;
/// Cognitive complexity.
pub mod cognitive;
/// Cyclomatic complexity.
pub mod cyclomatic;
/// Exit-point counting.
pub mod exit;
/// Halstead suite (operators, operands, volume, difficulty, effort).
pub mod halstead;
/// Lines-of-code variants (SLOC, PLOC, LLOC, CLOC, blank).
pub mod loc;
/// Maintainability Index.
pub mod mi;
/// Number of arguments per function.
pub mod nargs;
/// Number of methods (functions + closures).
pub mod nom;
/// Number of public attributes.
pub mod npa;
/// Number of public methods.
pub mod npm;
/// Token count.
pub mod tokens;
/// Weighted Methods per Class.
pub mod wmc;

/// Divides a metric sum by a count, guarding the divisor with `.max(1)`.
///
/// Every "average over a count" metric routes through this helper so the
/// divide-by-zero guard added for [#428] is applied uniformly rather than
/// per call site. A `count` of `0` degrades to `sum / 1` (the sum itself)
/// instead of producing `inf`/`NaN`, so a never-observed or count-less
/// space still serializes a finite number.
///
/// The *meaning* of `count` is the caller's choice of denominator
/// convention; the project uses two:
///
/// - **Per-function** averages (`cognitive`, `cyclomatic`, `exit`,
///   `nargs`) divide by the function/closure count of the subtree, so the
///   value reads as "average complexity per function". `cognitive`/
///   `exit`/`nargs` source this count from `Nom`; `cyclomatic` counts its
///   own function/closure *spaces* (equal in the common case, but
///   independent of whether `Nom` is selected — see
///   `cyclomatic::Stats::function_spaces`).
/// - **Per-space** averages (`nom`, `loc`, `abc`, `tokens`) divide by the
///   total number of spaces (functions, closures, classes, the file unit,
///   …). These measure a property of each space rather than of each
///   function, so a per-function denominator would not match their
///   meaning (and for `nom` it would be circular — it *is* the function
///   count).
///
/// [#428]: https://github.com/dekobon/big-code-analysis/issues/428
#[inline]
#[must_use]
// `count as f64` is exact for any realistic space count; the cast mirrors
// the per-metric modules' module-level allowance for count-to-float casts.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn average(sum: f64, count: usize) -> f64 {
    sum / count.max(1) as f64
}

/// Serialize wrapper that maps a non-finite `f64` (`NaN`/`±Infinity`) to a
/// null at the structured-output boundary, uniformly across every format.
///
/// A non-finite metric value means "not applicable" (an average or ratio
/// over an empty subtree, a Halstead score whose `log`/division was
/// undefined). Serde's default `f64` serialization renders those
/// inconsistently per format — `serde_json` emits `null`, `toml` emits
/// `nan`, `serde_yaml` emits `.nan`, and CBOR encodes the raw IEEE-754
/// bits — so the same metric reads differently depending on the chosen
/// output. Routing every float field through this newtype collapses that
/// divergence to a single policy enforced *once* at the serialize boundary
/// rather than relying on each accessor staying finite (the accessors are
/// guarded today — see [#428], [#438], and the Halstead/MI `log`/division
/// guards — but nothing structurally prevents a future metric from
/// reintroducing the silent split).
///
/// Finite values serialize unchanged via `serialize_f64`, so output for
/// every value a metric can currently produce is byte-identical to the
/// pre-wrapper behavior. Non-finite values serialize via `serialize_none`,
/// which yields a native `null` in JSON/YAML/CBOR and an omitted key in
/// TOML (TOML has no null literal). The serialized precision is full `f64`
/// precision; see `STABILITY.md` for the documented (non-byte-stable)
/// precision contract.
///
/// This is the structured-output (serde) arm of the "non-finite metric
/// means not-applicable" policy. The human-readable arm lives in
/// `crate::output::numfmt`: `CellMetric` renders non-finite as an empty
/// CSV cell and `MessageMetric` as a `Display` fallback. A change to the
/// policy should be mirrored across all three.
///
/// [#428]: https://github.com/dekobon/big-code-analysis/issues/428
/// [#438]: https://github.com/dekobon/big-code-analysis/issues/438
#[derive(Clone, Copy, Debug)]
pub(crate) struct NonFinite(pub(crate) f64);

impl serde::Serialize for NonFinite {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.0.is_finite() {
            serializer.serialize_f64(self.0)
        } else {
            serializer.serialize_none()
        }
    }
}

#[cfg(test)]
mod non_finite_tests {
    use super::NonFinite;
    use serde::Serialize;
    use serde::ser::{SerializeStruct, Serializer};

    /// A single-field struct so each format serializes `NonFinite` in the
    /// same struct-field position the real metric `Stats` impls use — TOML's
    /// "omit the key" behavior only applies to a struct/table field, not a
    /// bare top-level scalar.
    struct Wrap(NonFinite);

    impl Serialize for Wrap {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut st = serializer.serialize_struct("Wrap", 1)?;
            st.serialize_field("value", &self.0)?;
            st.end()
        }
    }

    /// Finite values must serialize as the bare number, byte-identical to a
    /// plain `f64`, in every format. The wrapper must be transparent for the
    /// values metrics actually produce today.
    #[test]
    fn finite_serializes_as_the_number_in_every_format() {
        let wrap = Wrap(NonFinite(1.5));

        assert_eq!(serde_json::to_string(&wrap).unwrap(), r#"{"value":1.5}"#);
        assert_eq!(serde_yaml::to_string(&wrap).unwrap(), "value: 1.5\n");
        assert_eq!(toml::to_string(&wrap).unwrap(), "value = 1.5\n");

        let mut cbor = Vec::new();
        serde_cbor::to_writer(&mut cbor, &wrap).unwrap();
        let value: serde_cbor::Value = serde_cbor::from_slice(&cbor).unwrap();
        let serde_cbor::Value::Map(map) = value else {
            panic!("CBOR root is not a map");
        };
        assert_eq!(
            map.get(&serde_cbor::Value::Text("value".to_owned())),
            Some(&serde_cbor::Value::Float(1.5)),
        );
    }

    /// Every non-finite value (`NaN`, `+Infinity`, `-Infinity`) must collapse
    /// to the format's null — native `null` in JSON/YAML/CBOR, an omitted key
    /// in TOML (which has no null literal). This is the uniform policy #531
    /// locks in: no silent `null`-vs-`nan`-vs-raw-bits split across formats.
    #[test]
    fn non_finite_collapses_to_null_or_omission_in_every_format() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let wrap = Wrap(NonFinite(value));

            // The JSON arm documents cross-format uniformity, not the
            // wrapper itself: `serde_json` already maps a bare non-finite
            // `f64` to `null`, so this assertion holds even against a
            // reverted `NonFinite` that just called `serialize_f64`. The
            // YAML/TOML/CBOR arms below are the actual regression guards —
            // each diverges (`.nan` / `nan` literal / float bits) under a
            // plain-`serialize_f64` revert and only collapses to null/omit
            // because `NonFinite` routes through `serialize_none`.
            assert_eq!(
                serde_json::to_string(&wrap).unwrap(),
                r#"{"value":null}"#,
                "JSON non-finite ({value}) must be null",
            );
            assert_eq!(
                serde_yaml::to_string(&wrap).unwrap(),
                "value: null\n",
                "YAML non-finite ({value}) must be null",
            );
            assert_eq!(
                toml::to_string(&wrap).unwrap(),
                "",
                "TOML non-finite ({value}) must omit the key (no null literal)",
            );

            let mut cbor = Vec::new();
            serde_cbor::to_writer(&mut cbor, &wrap).unwrap();
            let parsed: serde_cbor::Value = serde_cbor::from_slice(&cbor).unwrap();
            let serde_cbor::Value::Map(map) = parsed else {
                panic!("CBOR root is not a map");
            };
            assert_eq!(
                map.get(&serde_cbor::Value::Text("value".to_owned())),
                Some(&serde_cbor::Value::Null),
                "CBOR non-finite ({value}) must be null",
            );
        }
    }
}
