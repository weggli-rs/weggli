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

/// We use captures as a way to extend tree-sitter's query mechanism.
/// Variable captures correspond to a weggli variable ($foo) and we enforce
/// equality of a single variable for all queries in a tree.
/// Check is used for weggli identifiers such as variable or function names.
/// Finally, Subquery contains the QueryTree that needs to be executed on
/// the captured AST node.
#[derive(Debug)]
pub enum Capture {
    Display,
    Variable(String),
    Check(String),
    Subquery(Box<crate::query::QueryTree>),
}

pub fn add_capture(captures: &mut Vec<Capture>, capture: Capture) -> String {
    let idx = captures.len();
    captures.push(capture);
    idx.to_string()
}
