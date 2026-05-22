//! `serde_json::Value` → Python object conversion.
//!
//! The Python bindings reuse the library's existing `Serialize`
//! implementation on [`big_code_analysis::FuncSpace`]: each call to
//! [`crate::analysis::analyze`] / [`crate::analysis::analyze_source`]
//! runs `serde_json::to_value(&space)` and hands the resulting
//! [`serde_json::Value`] to [`json_value_to_py`] for conversion to
//! a Python `dict`.
//!
//! Routing through `serde_json::Value` rather than hand-mapping every
//! metric struct guarantees byte-for-byte parity with the CLI's
//! `bca metrics --output json` (which serialises the same `FuncSpace`
//! through `serde_json::to_string`). Any future field added to a
//! `Metrics` struct flows through automatically.

use pyo3::Bound;
use pyo3::IntoPyObject;
use pyo3::PyAny;
use pyo3::PyErr;
use pyo3::Python;
use pyo3::exceptions::PyValueError;
use pyo3::types::{PyDict, PyDictMethods, PyList, PyListMethods};
use serde_json::Value;

/// Recursively convert a [`serde_json::Value`] tree to a fresh Python
/// object owned by `py`.
///
/// The mapping is:
///
/// - `Null` → `None`
/// - `Bool(b)` → Python `bool`
/// - `Number(n)` with `is_i64()` → Python `int` (signed 64-bit fits)
/// - `Number(n)` with `is_u64()` → Python `int` (`CPython` ints are
///   arbitrary precision, so unsigned 64-bit fits)
/// - `Number(n)` otherwise → Python `float`
/// - `String(s)` → Python `str`
/// - `Array(xs)` → Python `list`
/// - `Object(map)` → Python `dict` (insertion order preserved by
///   both `serde_json` and `CPython` 3.7+, so the field order in the
///   `FuncSpace` `Serialize` impl is preserved)
///
/// # Errors
///
/// Returns `Err(PyValueError)` when a JSON number cannot be coerced
/// to either an `i64`/`u64` or a finite `f64`. In practice every
/// number produced by the metric serializers is one of those (NaN /
/// infinity are not emitted), so this branch is unreachable for
/// `FuncSpace` round-trips and exists purely as a defensive arm.
pub(crate) fn json_value_to_py<'py>(
    py: Python<'py>,
    value: &Value,
) -> Result<Bound<'py, PyAny>, PyErr> {
    match value {
        Value::Null => Ok(py.None().into_bound(py)),
        Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
        Value::Number(n) => json_number_to_py(py, n),
        Value::String(s) => Ok(s.into_pyobject(py)?.into_any()),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (key, val) in map {
                dict.set_item(key, json_value_to_py(py, val)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

/// Map a JSON number to the narrowest Python numeric type that
/// preserves its value.
///
/// `serde_json::Number` does not expose its bit pattern directly;
/// classify via the typed accessors instead. `CPython` integers are
/// arbitrary precision, so both `i64` and `u64` round-trip exactly.
fn json_number_to_py<'py>(
    py: Python<'py>,
    n: &serde_json::Number,
) -> Result<Bound<'py, PyAny>, PyErr> {
    if let Some(i) = n.as_i64() {
        return Ok(i.into_pyobject(py)?.into_any());
    }
    if let Some(u) = n.as_u64() {
        return Ok(u.into_pyobject(py)?.into_any());
    }
    if let Some(f) = n.as_f64() {
        return Ok(f.into_pyobject(py)?.into_any());
    }
    // serde_json's `Number` covers exactly the i64/u64/f64 union, so
    // this arm is unreachable in practice. Surface a Python-side
    // error rather than panicking.
    Err(PyValueError::new_err(format!(
        "JSON number out of range for Python conversion: {n}",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::{PyAnyMethods, PyBool, PyDict, PyFloat, PyInt, PyList, PyString};
    use serde_json::json;

    #[test]
    fn null_maps_to_none() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_value_to_py(py, &Value::Null).expect("null converts");
            assert!(obj.is_none());
        });
    }

    #[test]
    fn bool_maps_to_python_bool() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_value_to_py(py, &Value::Bool(true)).expect("bool converts");
            assert!(obj.is_instance_of::<PyBool>());
            assert!(obj.extract::<bool>().expect("extract bool"));
        });
    }

    #[test]
    fn signed_integer_maps_to_python_int() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_value_to_py(py, &json!(-42_i64)).expect("int converts");
            assert!(obj.is_instance_of::<PyInt>());
            assert_eq!(obj.extract::<i64>().expect("extract i64"), -42);
        });
    }

    #[test]
    fn unsigned_large_integer_maps_to_python_int() {
        Python::initialize();
        Python::attach(|py| {
            // u64::MAX exceeds i64::MAX — must still round-trip.
            let n = u64::MAX;
            let obj = json_value_to_py(py, &json!(n)).expect("u64 converts");
            assert!(obj.is_instance_of::<PyInt>());
            assert_eq!(obj.extract::<u64>().expect("extract u64"), n);
        });
    }

    #[test]
    fn float_maps_to_python_float() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_value_to_py(py, &json!(3.5_f64)).expect("float converts");
            assert!(obj.is_instance_of::<PyFloat>());
            let value: f64 = obj.extract().expect("extract f64");
            assert!((value - 3.5).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn string_maps_to_python_str() {
        Python::initialize();
        Python::attach(|py| {
            let obj =
                json_value_to_py(py, &Value::String("hello".to_owned())).expect("string converts");
            assert!(obj.is_instance_of::<PyString>());
            assert_eq!(obj.extract::<String>().expect("extract str"), "hello");
        });
    }

    #[test]
    fn array_maps_to_python_list_preserving_order() {
        Python::initialize();
        Python::attach(|py| {
            let obj = json_value_to_py(py, &json!([1, "two", null])).expect("array converts");
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
        Python::initialize();
        Python::attach(|py| {
            // Build an explicit ordered map so we can assert key order.
            let mut map = serde_json::Map::new();
            map.insert("first".to_owned(), json!(1));
            map.insert("second".to_owned(), json!(2));
            map.insert("third".to_owned(), json!(3));
            let obj = json_value_to_py(py, &Value::Object(map)).expect("object converts");
            assert!(obj.is_instance_of::<PyDict>());
            let dict = obj.cast_into::<PyDict>().expect("cast dict");
            let keys: Vec<String> = dict
                .keys()
                .iter()
                .map(|k| k.extract::<String>().expect("key str"))
                .collect();
            assert_eq!(keys, ["first", "second", "third"]);
        });
    }

    #[test]
    fn nested_structure_round_trips_field_order() {
        // Mirror the FuncSpace shape: name (str), start_line (int),
        // end_line (int), kind (str), spaces (list), metrics (dict).
        Python::initialize();
        Python::attach(|py| {
            let sample = json!({
                "name": "snippet.rs",
                "start_line": 1,
                "end_line": 10,
                "kind": "unit",
                "spaces": [],
                "metrics": {
                    "nargs": { "total_functions": 0 },
                }
            });
            let obj = json_value_to_py(py, &sample).expect("nested converts");
            let dict = obj.cast_into::<PyDict>().expect("cast top dict");
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
