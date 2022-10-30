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

use std::collections::{hash_map::Keys, HashMap};

use colored::Colorize;
use query::QueryTree;
use regex::Regex;
use tree_sitter::{Language, Parser, Query, Tree};

#[macro_use]
extern crate log;

pub mod builder;
mod capture;
mod util;

#[cfg(feature = "python")]
pub mod python;
pub mod query;
pub mod result;

extern "C" {
    fn tree_sitter_c() -> Language;
    fn tree_sitter_cpp() -> Language;
}

#[derive(Debug, Clone)]
pub struct QueryError {
    pub message: String,
}

/// Helper function to parse an input string
/// into a tree-sitter tree, using our own slightly modified
/// C grammar. This function won't fail but the returned
/// Tree might be invalid and contain errors.
pub fn parse(source: &str, cpp: bool) -> Tree {
    let mut parser = get_parser(cpp);
    parser.parse(source, None).unwrap()
}

pub fn get_parser(cpp: bool) -> Parser {
    let language = if !cpp {
        unsafe { tree_sitter_c() }
    } else {
        unsafe { tree_sitter_cpp() }
    };

    let mut parser  = Parser::new();
    if let Err(e) = parser.set_language(language) {
        eprintln!("{}", e);
        panic!();
    }
    parser
}

// Internal helper function to create a new tree-sitter query.
fn ts_query(sexpr: &str, cpp: bool) -> Result<tree_sitter::Query, QueryError> {
    let language = if !cpp {
        unsafe { tree_sitter_c() }
    } else {
        unsafe { tree_sitter_cpp() }
    };

    match Query::new(language, sexpr) {
        Ok(q) => Ok(q),
        Err(e) => {
            let errmsg = format!( "Tree sitter query generation failed: {:?}\n {} \n sexpr: {}\n This is a bug! Can't recover :/", e.kind, e.message, sexpr);
            Err(QueryError { message: errmsg })
        }
    }
}

/// Map from variable names to a positive/negative regex constraint
/// see --regex
#[derive(Clone)]
pub struct RegexMap(HashMap<String, (bool, Regex)>);

impl RegexMap {
    pub fn new(m: HashMap<String, (bool, Regex)>) -> RegexMap {
        RegexMap(m)
    }

    pub fn variables(&self) -> Keys<String, (bool, Regex)> {
        self.0.keys()
    }

    pub fn get(&self, variable: &str) -> Option<(bool, Regex)> {
        if let Some((b, r)) = self.0.get(variable) {
            Some((*b, r.to_owned()))
        } else {
            None
        }
    }
}

/// Translate the search pattern in `pattern` into a weggli QueryTree.
/// `is_cpp` enables C++ mode. `force_query` can be used to allow queries with syntax errors.
/// We support some basic normalization (adding { } around queries) and store the normalized form
/// in `normalized_patterns` to avoid lifetime issues.
pub fn parse_search_pattern(
    pattern: &str,
    is_cpp: bool,
    force_query: bool,
    regex_constraints: Option<RegexMap>,
) -> Result<QueryTree, QueryError> {
    let mut tree = parse(pattern, is_cpp);
    let mut p = pattern;

    let temp_pattern;

    // Try to fix missing ';' at the end of a query.
    // weggli 'memcpy(a,b,size)' should work.
    if tree.root_node().has_error() && !pattern.ends_with(';') {
        temp_pattern = format!("{};", &p);
        let fixed_tree = parse(&temp_pattern, is_cpp);
        if !fixed_tree.root_node().has_error() {
            info!("normalizing query: add missing ;");
            tree = fixed_tree;
            p = &temp_pattern;
        }
    }

    let temp_pattern2;

    // Try to do query normalization to support missing { }
    // 'memcpy(_);' -> {memcpy(_);}
    if !tree.root_node().has_error() {
        let c = tree.root_node().child(0);
        if let Some(n) = c {
            if !VALID_NODE_KINDS.contains(&n.kind()) {
                temp_pattern2 = format!("{{{}}}", &p);
                let fixed_tree = parse(&temp_pattern2, is_cpp);
                if !fixed_tree.root_node().has_error() {
                    info!("normalizing query: add {}", "{}");
                    tree = fixed_tree;
                    p = &temp_pattern2;
                }
            }
        }
    }

    let mut c = validate_query(&tree, p, force_query)?;

    builder::build_query_tree(p, &mut c, is_cpp, regex_constraints)
}

/// Supported root node types.
const VALID_NODE_KINDS: &[&str] = &[
    "compound_statement",
    "function_definition",
    "struct_specifier",
    "enum_specifier",
    "union_specifier",
    "class_specifier",
];

/// Validates the user supplied search query and quits with an error message in case
/// it contains syntax errors or isn't rooted in one of `VALID_NODE_KINDS`
/// If `force` is true, syntax errors are ignored. Returns a cursor to the
/// root node.
fn validate_query<'a>(
    tree: &'a tree_sitter::Tree,
    query: &str,
    force: bool,
) -> Result<tree_sitter::TreeCursor<'a>, QueryError> {
    if tree.root_node().has_error() && !force {
        let mut errmsg = format!("{}", "Error! Query parsing failed:".red().bold());
        let mut cursor = tree.root_node().walk();

        let mut first_error = None;
        loop {
            let node = cursor.node();
            if node.has_error() {
                if node.is_error() || node.is_missing() {
                    first_error = Some(node);
                    break;
                } else if !cursor.goto_first_child() {
                    break;
                }
            } else if !cursor.goto_next_sibling() {
                break;
            }
        }

        if let Some(node) = first_error {
            errmsg.push_str(&format!(" {}", &query[0..node.start_byte()].italic()));
            if node.is_missing() {
                errmsg.push_str(&format!(
                    "{}{}{}",
                    " [MISSING ".red(),
                    node.kind().red().bold(),
                    " ] ".red()
                ));
            }
            errmsg.push_str(&format!(
                "{}{}",
                &query[node.start_byte()..node.end_byte()]
                    .red()
                    .italic()
                    .bold(),
                &query[node.end_byte()..].italic()
            ));
        }

        return Err(QueryError { message: errmsg });
    }

    info!("query sexp: {}", tree.root_node().to_sexp());

    let mut c = tree.walk();

    if c.node().named_child_count() > 1 {
        return Err(QueryError {
            message: format!(
                "{}'{}' query contains multiple root nodes",
                "Error: ".red(),
                query
            ),
        });
    }

    c.goto_first_child();

    if !VALID_NODE_KINDS.contains(&c.node().kind()) {
        return Err(QueryError {
            message: format!(
                "{}'{}' is not a supported query root node.",
                "Error: ".red(),
                query
            ),
        });
    }

    Ok(c)
}
