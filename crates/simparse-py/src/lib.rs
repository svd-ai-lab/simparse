use std::path::PathBuf;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use simparse_core::{InspectOptions, ScanOptions, SimFormat, inspect_path, scan_paths};

#[pyfunction]
#[pyo3(signature = (path, format = "auto", include_paths = false))]
fn inspect(py: Python<'_>, path: &str, format: &str, include_paths: bool) -> PyResult<Py<PyAny>> {
    let options = InspectOptions {
        format: parse_format(format)?,
        include_paths,
        max_text_bytes: InspectOptions::default().max_text_bytes,
    };
    let result = inspect_path(path, options).map_err(to_py_err)?;
    json_to_py(py, &result)
}

#[pyfunction]
#[pyo3(signature = (paths, recursive = true, include_paths = false))]
fn scan(
    py: Python<'_>,
    paths: Vec<String>,
    recursive: bool,
    include_paths: bool,
) -> PyResult<Py<PyAny>> {
    let options = ScanOptions {
        recursive,
        include_paths,
        inspect: InspectOptions {
            include_paths,
            ..InspectOptions::default()
        },
        ..ScanOptions::default()
    };
    let paths: Vec<_> = paths.into_iter().map(PathBuf::from).collect();
    let result = scan_paths(&paths, options).map_err(to_py_err)?;
    json_to_py(py, &result)
}

#[pymodule]
fn _simparse(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(inspect, m)?)?;
    m.add_function(wrap_pyfunction!(scan, m)?)?;
    Ok(())
}

fn parse_format(value: &str) -> PyResult<Option<SimFormat>> {
    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    value
        .parse::<SimFormat>()
        .map(Some)
        .map_err(PyRuntimeError::new_err)
}

fn json_to_py<T: serde::Serialize>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>> {
    let text = serde_json::to_string(value).map_err(to_py_err)?;
    let json = py.import("json")?;
    let obj = json.call_method1("loads", (text,))?;
    Ok(obj.unbind())
}

fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}
