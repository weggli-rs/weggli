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
    let language = if !cpp {
        unsafe { tree_sitter_c() }
    } else {
        unsafe { tree_sitter_cpp() }
    };
    let mut parser = Parser::new();
    if let Err(e) = parser.set_language(language) {
        eprintln!("{}", e);
        panic!();
    }

    parser.parse(source, None).unwrap()
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
