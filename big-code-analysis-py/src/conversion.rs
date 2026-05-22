//! JSON-string → Python object conversion.
//!
//! The Python bindings reuse the library's existing `Serialize`
//! implementation on [`big_code_analysis::FuncSpace`]: each call to
//! [`crate::analysis::analyze_path`] /
//! [`crate::analysis::analyze_source`] runs
//! `serde_json::to_string(&space)` and hands the resulting JSON
//! `String` to [`json_string_to_py`], which parses it with `CPython`'s
//! standard-library `json.loads`.
//!
//! Routing through the same `serde_json::to_string` serializer the
//! `bca` CLI uses — combined with `CPython` 3.7+ dict insertion-order
//! preservation in `json.loads` — gives byte-for-byte parity with
//! `bca metrics --output-format json` for the same input: identical
//! field order (`name`, `start_line`, `end_line`, `kind`, `spaces`,
//! `metrics`), identical numeric formatting (int vs float), and any
//! future field added to a `Metrics` struct flows through
//! automatically.
//!
//! The earlier `serde_json::to_value` + recursive `Value` → Python
//! conversion produced *structurally* equivalent output but silently
//! sorted keys alphabetically (`serde_json::Map` is a `BTreeMap`
//! without the `preserve_order` Cargo feature, which the workspace
//! does not enable), so it could not honour the byte-for-byte
//! contract.

use pyo3::Bound;
use pyo3::PyAny;
use pyo3::PyErr;
use pyo3::Python;
use pyo3::intern;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyModule;

/// Parse a JSON `&str` produced by `serde_json::to_string` into the
/// equivalent Python object, using `CPython`'s standard-library
/// `json.loads`.
///
/// `json.loads` builds Python `dict`s in input order (`CPython`
/// 3.7+ dicts preserve insertion order), which is what makes the
/// final Python output byte-for-byte equivalent — modulo type
/// mapping — to the JSON the `bca` CLI prints.
///
/// # Errors
///
/// Surfaces any exception raised by `json.loads`. In practice that
/// can only fire on malformed input, which would itself be an
/// internal bug — `serde_json::to_string` is the only producer.
pub(crate) fn json_string_to_py<'py>(
    py: Python<'py>,
    json: &str,
) -> Result<Bound<'py, PyAny>, PyErr> {
    PyModule::import(py, intern!(py, "json"))?.call_method1(intern!(py, "loads"), (json,))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::{
        PyBool, PyDict, PyDictMethods, PyFloat, PyInt, PyList, PyListMethods, PyString,
    };

    #[test]
    fn null_maps_to_none() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "null").expect("null parses");
            assert!(obj.is_none());
        });
    }

    #[test]
    fn bool_maps_to_python_bool() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "true").expect("bool parses");
            assert!(obj.is_instance_of::<PyBool>());
            assert!(obj.extract::<bool>().expect("extract bool"));
        });
    }

    #[test]
    fn signed_integer_maps_to_python_int() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "-42").expect("int parses");
            assert!(obj.is_instance_of::<PyInt>());
            assert_eq!(obj.extract::<i64>().expect("extract i64"), -42);
        });
    }

    #[test]
    fn unsigned_large_integer_maps_to_python_int() {
        Python::initialize();
        Python::attach(|py| {
            // u64::MAX exceeds i64::MAX. Python ints are arbitrary
            // precision, so this must round-trip exactly.
            let n: u64 = u64::MAX;
            let obj = json_string_to_py(py, &n.to_string()).expect("u64 parses");
            assert!(obj.is_instance_of::<PyInt>());
            assert_eq!(obj.extract::<u64>().expect("extract u64"), n);
        });
    }

    #[test]
    fn float_maps_to_python_float() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "3.5").expect("float parses");
            assert!(obj.is_instance_of::<PyFloat>());
            let value: f64 = obj.extract().expect("extract f64");
            assert!((value - 3.5).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn string_maps_to_python_str() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "\"hello\"").expect("string parses");
            assert!(obj.is_instance_of::<PyString>());
            assert_eq!(obj.extract::<String>().expect("extract str"), "hello");
        });
    }

    #[test]
    fn array_maps_to_python_list_preserving_order() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_string_to_py(py, "[1, \"two\", null]").expect("array parses");
            assert!(obj.is_instance_of::<PyList>());
            let list = obj.cast_into::<PyList>().expect("cast list");
            assert_eq!(list.len(), 3);
            assert_eq!(
                list.get_item(0)
                    .expect("idx 0")
                    .extract::<i64>()
                    .expect("i64"),
                1,
            );
            assert_eq!(
                list.get_item(1)
                    .expect("idx 1")
                    .extract::<String>()
                    .expect("str"),
                "two",
            );
            assert!(list.get_item(2).expect("idx 2").is_none());
        });
    }

    #[test]
    fn object_maps_to_python_dict_preserving_insertion_order() {
        // Build a JSON object whose source order is explicitly
        // *non*-alphabetical. Because `json_string_to_py` parses
        // through `CPython`'s `json.loads`, and `CPython` 3.7+
        // `dict`s preserve insertion order, the observed key order
        // in Python is the source order — NOT alphabetical.
        //
        // This is the test that fails if anyone re-introduces the
        // old `serde_json::to_value` path (which silently sorts keys
        // alphabetically through `BTreeMap`).
        Python::initialize();
        Python::attach(|py| {
            let src = r#"{"zeta": 1, "alpha": 2, "mu": 3}"#;
            let obj = json_string_to_py(py, src).expect("object parses");
            assert!(obj.is_instance_of::<PyDict>());
            let dict = obj.cast_into::<PyDict>().expect("cast dict");
            let keys: Vec<String> = dict
                .keys()
                .iter()
                .map(|k| k.extract::<String>().expect("key str"))
                .collect();
            assert_eq!(keys, ["zeta", "alpha", "mu"]);
        });
    }

    #[test]
    fn nested_structure_preserves_funcspace_field_order() {
        // The whole reason this layer exists: when the upstream
        // `FuncSpace` `Serialize` impl emits keys in declaration
        // order (`name`, `start_line`, `end_line`, `kind`, `spaces`,
        // `metrics`), that same order must reach Python intact. This
        // is what makes the bindings' output byte-for-byte parity
        // with `bca metrics --output-format json` actually true.
        Python::initialize();
        Python::attach(|py| {
            let src = r#"{
                "name": "snippet.rs",
                "start_line": 1,
                "end_line": 10,
                "kind": "unit",
                "spaces": [],
                "metrics": {"nargs": {"total_functions": 0}}
            }"#;
            let obj = json_string_to_py(py, src).expect("nested parses");
            let dict = obj.cast_into::<PyDict>().expect("cast top dict");
            let keys: Vec<String> = dict
                .keys()
                .iter()
                .map(|k| k.extract::<String>().expect("key str"))
                .collect();
            assert_eq!(
                keys,
                [
                    "name",
                    "start_line",
                    "end_line",
                    "kind",
                    "spaces",
                    "metrics"
                ],
                "observed key order must match FuncSpace Serialize declaration order",
            );
            assert_eq!(
                dict.get_item("name")
                    .expect("get name")
                    .expect("name present")
                    .extract::<String>()
                    .expect("name str"),
                "snippet.rs",
            );
            let metrics = dict
                .get_item("metrics")
                .expect("get metrics")
                .expect("metrics present")
                .cast_into::<PyDict>()
                .expect("metrics dict");
            assert!(metrics.contains("nargs").expect("contains nargs"));
        });
    }
}
