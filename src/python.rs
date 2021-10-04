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

use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use crate::query::QueryTree;
use crate::result::QueryResult;

#[pyclass]
struct QueryTreePy {
    qt: QueryTree,
}

#[pyclass]
struct QueryResultPy {
    qr: QueryResult,
}

#[pyfunction]
fn parse_query(q: &str) -> PyResult<QueryTreePy> {
    let tree = crate::parse(q, false);
    let mut c = tree.walk();

    let qt = crate::builder::build_query_tree(q, &mut c, false);
    Ok(QueryTreePy { qt })
}

#[pyfunction]
fn identifiers(p: &QueryTreePy) -> PyResult<Vec<String>> {
    Ok(p.qt.identifiers())
}

#[pyfunction]
fn matches(p: &QueryTreePy, source: &str) -> PyResult<Vec<QueryResultPy>> {
    let source_tree = crate::parse(source, false);

    let matches = p.qt.matches(source_tree.root_node(), source);

    let r = matches.into_iter().map(|qr| QueryResultPy { qr }).collect();

    Ok(r)
}

#[pyfunction]
fn display(p: &QueryResultPy, source: &str) -> PyResult<String> {
    let r = p.qr.display(source, 10, 10);
    println!("{}", r.as_bytes().len());
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
