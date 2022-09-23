/*
Copyright 2021 Google LLC

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use crate::query::QueryTree;
use crate::result::QueryResult;
use crate::QueryError;

impl std::convert::From<QueryError> for PyErr {
    fn from(err: QueryError) -> PyErr {
        PyValueError::new_err(err.message)
    }
}

#[pyclass]
struct QueryTreePy {
    qt: QueryTree,
}

#[pyclass]
struct QueryResultPy {
    qr: QueryResult,
}

#[pyfunction(cpp = "false")]
#[pyo3(text_signature = "(query, cpp)")]
fn parse_query(q: &str, cpp: bool) -> PyResult<QueryTreePy> {
    let tree = crate::parse(q, cpp);
    let mut c = tree.walk();

    let qt = crate::builder::build_query_tree(q, &mut c, cpp, None)?;
    Ok(QueryTreePy { qt })
}

#[pyfunction]
#[pyo3(text_signature = "(p)")]
fn identifiers(p: &QueryTreePy) -> PyResult<Vec<String>> {
    Ok(p.qt.identifiers())
}

#[pyfunction(cpp = "false")]
#[pyo3(text_signature = "(p, source, cpp)")]
fn matches(p: &QueryTreePy, source: &str, cpp: bool) -> PyResult<Vec<QueryResultPy>> {
    let source_tree = crate::parse(source, cpp);

    let matches = p.qt.matches(source_tree.root_node(), source);

    let r = matches.into_iter().map(|qr| QueryResultPy { qr }).collect();

    Ok(r)
}

#[pyfunction(color = "None")]
#[pyo3(text_signature = "(q, source, color)")]
fn display(p: &QueryResultPy, source: &str, color: Option<bool>) -> PyResult<String> {
    if let Some(color_override) = color {
        colored::control::set_override(color_override);
    }
    let r = p.qr.display(source, 10, 10);
    colored::control::unset_override();
    Ok(r)
}

#[pymodule]
fn weggli(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<QueryTreePy>()?;
    m.add_function(wrap_pyfunction!(parse_query, m)?)?;
    m.add_function(wrap_pyfunction!(identifiers, m)?)?;
    m.add_function(wrap_pyfunction!(matches, m)?)?;
    m.add_function(wrap_pyfunction!(display, m)?)?;

    Ok(())
}
