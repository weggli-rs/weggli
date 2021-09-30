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

use std::collections::{HashMap, HashSet};

use crate::capture::{add_capture, Capture};
use crate::query::{NegativeQuery, QueryTree};
use tree_sitter::{Node, TreeCursor};

/// Translate a parsed and validated input source (specified by `source` and `cursor`) into a `QueryTree`.
/// When `is_cpp` is set, C++ specific features are enabled.
pub fn build_query_tree(source: &str, cursor: &mut TreeCursor, is_cpp: bool) -> QueryTree {
    _build_query_tree(source, cursor, 0, is_cpp, false)
}

fn _build_query_tree(
    source: &str,
    c: &mut TreeCursor,
    id: usize,
    is_cpp: bool,
    is_multi_pattern: bool,
) -> QueryTree {
    let mut b = QueryBuilder {
        query_source: source.to_string(),
        captures: Vec::new(),
        negations: Vec::new(),
        id,
        cpp: is_cpp,
    };

    // Skip the root node if it's a translation_unit.
    if c.node().kind() == "translation_unit" {
        debug!("query cursor specifies translation_unit");
        c.goto_first_child();
    }

    let mut variables = HashSet::new();

    let sexp = if !is_multi_pattern {
        // We want to wrap queries into a function_definition so we can easily
        // extract the function that contains a match. Of course we should not do that
        // if the user specifies a function_definition as part of the query.
        let needs_anchor = c.node().kind() == "compound_statement" && id == 0;
        debug!("query needs anchor: {}", needs_anchor);

        // The main work happens here. Iterate through the AST and create a tree-sitter query
        let mut s = b.build(c, 0);

        // Make sure user supplied function headers are displayed by adding a Capture
        if !needs_anchor {
            s += "@";
            s += &add_capture(&mut b.captures, Capture::Display);
        }

        // Iterate through all captures, add their constraints to the query and extract used variables
        s += &process_captures(&b.captures, 0, &mut variables);

        // Optionally anchor query with a function_definition
        if needs_anchor {
            let capture = Capture::Display;
            format!(
                "(function_definition body: {}) @{}",
                s,
                &add_capture(&mut b.captures, capture)
            )
        } else {
            "(".to_string() + &s + ")"
        }
    } else {
        // When building a QueryTree for a compound statement, we create a tree-sitter
        // query with multiple root patterns for efficient searching.
        // This code is only executed when creating sub queries so we can skip
        // the whole anchoring logic needed for the single pattern case.

        assert!(c.goto_first_child());
        assert!(c.goto_next_sibling());

        let mut s = String::new();
        loop {
            let child = c.node();
            if !c.goto_next_sibling() {
                break;
            }

            let before = b.captures.len();
            let mut cursor = child.walk();

            let child_sexp = b.build(&mut cursor, 0);

            let captures = &process_captures(&b.captures, before, &mut variables);

            if !child_sexp.is_empty() {
                s += &format!("({} {})", child_sexp, captures);
            }
        }
        s
    };

    debug!("tree_sitter query {}: {}", id, sexp);

    QueryTree::new(
        crate::ts_query(&sexp, is_cpp),
        b.captures,
        variables,
        b.negations,
        source.to_string(),
        id,
    )
}

/// Iterates through `captures` starting at `offset` and returns the necessary query predicates as a string.
/// In addition, all captured variables are added to the `variables` set.
///
/// For constant captures (such as function or variable names), `process_captures` creates a equality predicate
/// (#eq @0 "memcpy"). For variables, we enforce equality between two occurences of the same variable (#eq @0 @1)
fn process_captures(
    captures: &[Capture],
    offset: usize,
    variables: &mut HashSet<String>,
) -> String {
    // HashMap to store the capture indexes of each variable.
    let mut vars: HashMap<String, Vec<usize>> = HashMap::new();
    // tree-sitter query predicates
    let mut sexp = String::new();

    // Note that we need offset to assign the right names to our
    // capture predicates. So simply passing captures[offset..] to
    // the function would not work.
    for (i, c) in captures.iter().skip(offset).enumerate() {
        match c {
            Capture::Display => (),
            Capture::Check(s) => {
                sexp += &format!(r#"(#eq? @{} "{}")"#, (i + offset).to_string(), s);
            }
            Capture::Variable(var) => {
                vars.entry(var.clone())
                    .or_insert_with(Vec::new)
                    .push(i + offset);

                // Add var to our result set
                variables.insert(var.clone());
            }
            _ => (),
        }
    }

    // Create equality predicates for all captures pointing at the same variable
    for (_, vec) in vars.iter() {
        if vec.len() > 1 {
            let a = vec[0].to_string();
            for capture in vec.iter().skip(1) {
                let b = capture.to_string();
                sexp += &format!(r#"(#eq? @{} @{})"#, a.to_string(), b.to_string());
            }
        }
    }

    sexp
}

/// `QueryBuilder` keeps the state we need while building queries.
struct QueryBuilder {
    query_source: String,
    captures: Vec<Capture>, // captures such as variables ($x), constants (memcpy) or sub queries
    negations: Vec<NegativeQuery>, // all negative sub queries (not: )
    id: usize,              // a globally unique ID used for caching results see `query.rs`
    cpp: bool,              // flag to enable C++ support
}

impl QueryBuilder {
    // Map from an AST node to its input source
    fn get_text(&self, n: &tree_sitter::Node) -> &str {
        &self.query_source[n.byte_range()]
    }

    // Returns true iff `query` is a wildcard function call _(..)
    fn is_subexpr_wildcard(&self, query: Node) -> bool {
        if query.kind() != "call_expression" {
            return false;
        }

        let f = query.child_by_field_name("function").unwrap();
        if f.utf8_text(self.query_source.as_bytes()).unwrap() == "_" {
            return true;
        }
        false
    }

    /// Translate the tree below `c` into a tree-sitter query string.
    /// This function is responsible for the weggli's greediness by turning
    /// the fixed input AST into a tree-sitter query that can match on different but related
    /// AST's in the queried source code. Besides returning the query string, `build` will
    /// also add captures and negations to the active QueryBuilder.
    fn build(&mut self, c: &mut TreeCursor, depth: usize) -> String {
        // This function works by recursively processing every node in the tree,
        // creating new sub queries, captures or negative queries when needed
        // and slowly constructing the final tree-sitter query (note that query predicates are only
        // added in build_query_tree after this function returns)
        // Query building isn't performance critical as it's only done once at program startup, so
        // we don't have to worry much about optimizing this code.

        // Anonymous nodes are string constants like "+" or "+=".
        // We can simply copy them into the query.
        if !c.node().is_named() {
            return format!(r#""{}""#, c.node().kind().to_string());
        }

        let kind = c.node().kind();

        // First handle special cases. Note the implicit fallthroughs to the
        // default case after this match statement.
        match kind {
            // Handle not: xyz;
            "labeled_statement" => {
                let label = c.node().child(0).unwrap();
                if self.get_text(&label).to_uppercase() == "NOT" {
                    self.build_negative_query(c);
                    // negative sub queries are special in that they do not add anything
                    // to the main query. We just return an empty string, which will get
                    // filtered out by _build_query_tree
                    return "".to_string();
                }
            }
            // Build a multi-pattern tree for {.., .., ..}
            "compound_statement" => {
                self.id += 1;
                let mut c = c.node().walk();
                let capture = Capture::Subquery(Box::new(_build_query_tree(
                    &self.query_source,
                    &mut c,
                    self.id,
                    self.cpp,
                    true,
                )));
                return "(compound_statement) @".to_string()
                    + &add_capture(&mut self.captures, capture);
            }
            // Greedy matching of all type of identifiers + variable support
            "identifier"
            | "type_identifier"
            | "field_identifier"
            | "sized_type_specifier"
            | "primitive_type"
            | "namespace_identifier" => return self.build_identifier(c),
            "assignment_expression" => return self.build_assignment(c, depth),
            // Function calls (including wildcards)
            "call_expression" => match self.build_call_expr(c, depth) {
                Some(s) => return s,
                _ => (),
            },
            // When the query contains an expression statement (e.g "func(x,y);")
            // we insert a sub query for the expression instead. This ensures that
            // we also match on x=func(x,y); or if (func(x,y))
            "expression_statement" => {
                c.goto_first_child();
                return self.build(c, depth);
            }
            _ => (),
        }

        // Default case. Handle everything else

        // Enforce ordering of arguments by anchoring them to each other if the user specified
        // more than one arg.
        let anchoring = kind == "argument_list" && c.node().named_child_count() > 1;

        let is_funcdef = kind == "function_definition";

        let mut result = format!("({}", c.node().kind());
        if !c.goto_first_child() {
            if !c.node().is_named() {
                return format!(r#""{}""#, c.node().kind().to_string());
            }
            return result + ")";
        }

        // Iterate through all fields
        loop {
            let name = c.field_name();

            // Named fields (for example "condition" and "consequence" for an if statement)
            if let Some(n) = name {
                result += &format!(" {}:", n);

                // Recursively build the query for the child node.
                let t = self.build(c, depth + 1);

                if n == "declarator" && is_funcdef {
                    // hacky way to make "_ func()" match on "bar * func()".
                    // The problem is that the pointer isn't part of the return
                    // type but is a pointer_declaration wrapper
                    // around the function_definition. We add a single level wildcard
                    // to still match, but of course that still fails for bar** func() :/
                    // TODO: Think about better ways to implement this, maybe we should just add another sub expression
                    result += &format!("([(_ {}) ({})])", t, t);
                } else {
                    result += &t
                }

            // Argument Lists for function calls
            } else if c.node().is_named() {
                if anchoring {
                    result += " .";
                }
                result += " ";
                result += &self.build(c, depth + 1);
            }

            if !c.goto_next_sibling() {
                break;
            }
        }
        c.goto_parent();

        debug!("generated query: {}", result);
        result + ")"
    }

    // Create a negative query matching the statement after
    // a NOT: label.
    fn build_negative_query(&mut self, c: &mut TreeCursor) {
        let negated_query = c.node().child(2).unwrap();
        // Save a reference to the previous capture so
        // query.rs can later enforce ordering
        let before = self.captures.len() as i64 - 1;

        self.id += 1;
        self.negations.push(NegativeQuery {
            qt: Box::new(_build_query_tree(
                &self.query_source,
                &mut negated_query.walk(),
                self.id,
                self.cpp,
                false,
            )),
            previous_capture_index: before,
        });
    }

    // Handle $x, _, foo, char, ->field and co.
    fn build_identifier(&mut self, c: &mut TreeCursor) -> String {
        let pattern = self.get_text(&c.node());
        let kind = c.node().kind();

        if pattern == "_" {
            return "(_)".to_string();
        }

        let mut result = if kind == "type_identifier" {
            "[ (type_identifier) (sized_type_specifier) (primitive_type)]".to_string()
        } else if kind == "identifier" && pattern.starts_with('$') {
            if self.cpp {
                "[(identifier) (field_expression) (field_identifier) (scoped_identifier)]"
                    .to_string()
            } else {
                "[(identifier) (field_expression) (field_identifier)]".to_string()
            }
        } else {
            format!("({})", kind)
        };

        let capture = if pattern.starts_with('$') {
            Capture::Variable(pattern.to_string())
        } else {
            Capture::Check(pattern.to_string())
        };

        result += " @";
        result += &add_capture(&mut self.captures, capture);

        return result;
    }

    // Handle $foo() and _(). Returns None if the call does not need special handling.
    fn build_call_expr(&mut self, c: &mut TreeCursor, depth: usize) -> Option<String> {
        if self.is_subexpr_wildcard(c.node()) {
            let mut arg = c.node().child_by_field_name("arguments").unwrap().walk();
            arg.goto_first_child();
            arg.goto_next_sibling();

            // Wildcards for depth 0 are meaningless. Just unwrap it.
            if depth == 0 {
                return Some(self.build(&mut arg, depth));
            }
            self.id += 1;
            let capture = Capture::Subquery(Box::new(_build_query_tree(
                &self.query_source,
                &mut arg,
                self.id,
                self.cpp,
                false,
            )));
            return Some("_ @".to_string() + &add_capture(&mut self.captures, capture));
        }
        let function = c.node().child_by_field_name("function").unwrap();
        let arguments = c.node().child_by_field_name("arguments").unwrap();

        if function.kind() == "identifier" {
            let pattern = self.get_text(&function);
            if !pattern.starts_with('$') {
                let capture = Capture::Check(pattern.to_string());

                let capture_str = "@".to_string() + &add_capture(&mut self.captures, capture);

                let a = self.build(&mut arguments.walk(), depth + 1);

                let fs = if self.cpp {
                    format! {"[(field_expression field: (field_identifier){0})
                    (scoped_identifier name: (identifier){0}) (identifier) {0}]",capture_str}
                } else {
                    format! {"[(field_expression field: (field_identifier){0})
                    (identifier) {0}]",capture_str}
                };

                let result = format! {"(call_expression function: {} arguments: {})", fs, a};
                return Some(result);
            }
        }
        None
    }

    // Handle $x = .., $y+= .. etc.
    fn build_assignment(&mut self, c: &mut TreeCursor, depth: usize) -> String {
        assert!(c.goto_first_child());
        let left = self.build(c, depth + 1);

        let left_is_identifier = c.node().kind() == "identifier";

        // operator
        assert!(c.goto_next_sibling());

        // Match on assignments even if they include a cast
        let optional_cast = |r: String| format! {"[(cast_expression value: {}) {}]", r, r};

        // handle += / -= / ..
        let result = if c.node().kind() != "=" || !left_is_identifier {
            let operator = self.build(c, depth + 1);
            assert!(c.goto_next_sibling());
            let right = optional_cast(self.build(c, depth + 1));

            format! {"(assignment_expression left: {} {} right: {})" , left, operator, right}
        } else {
            // A query that searches for assignments (a = x;) should also match on init declarations (int a =x;)
            assert!(c.goto_next_sibling());
            let right = optional_cast(self.build(c, depth + 1));

            format! {r"[(assignment_expression left: {0} right: {1})
                        (init_declarator declarator: {0} value: {1}) 
                        (init_declarator declarator:(pointer_declarator declarator: {0}) value: {1})]", left,right}
        };
        c.goto_parent();
        return result;
    }
}
