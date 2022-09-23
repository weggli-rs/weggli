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

use rustc_hash::FxHashMap;
use std::collections::HashSet;
use tree_sitter::{Node, Query};

use crate::capture::Capture;
use crate::result::{CaptureResult, QueryResult};
use crate::util::parse_number_literal;

/// A query tree is our internal representation of a weggli search query.
/// tree-sitter's query syntax does not support all features that we need so
/// one weggli query will be split up into a tree of sub-queries, each
/// with it's own captures and variables.
#[derive(Debug)]
pub struct QueryTree {
    query: Query,
    captures: Vec<Capture>,
    negations: Vec<NegativeQuery>,
    variables: HashSet<String>,
    id: usize,
}

/// An internal cache for memoization of subquery results.
type Cache = FxHashMap<CacheKey, Vec<QueryResult>>;

/// Negative Queries are used to implement the not: feature.
/// In addition to the QueryTree we also store the
/// index of the previous capture in the parent query to enforce
/// ordering later on. (e.g a match for the negative query is only valid
/// if it comes AFTER the previous capture)
#[derive(Debug)]
pub struct NegativeQuery {
    pub qt: Box<QueryTree>,
    pub previous_capture_index: i64,
}

// Identify cache entries by the query id and the queried node.
#[derive(PartialEq, Eq, Hash, Clone)]
struct CacheKey {
    query_id: usize,
    node_id: usize,
}

impl QueryTree {
    pub fn new(
        query: Query,
        captures: Vec<Capture>,
        variables: HashSet<String>,
        negations: Vec<NegativeQuery>,
        id: usize,
    ) -> QueryTree {
        QueryTree {
            query,
            captures,
            variables,
            negations,
            id,
        }
    }

    /// Return all query variables used in a query.
    pub fn variables(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        for c in &self.captures {
            match c {
                Capture::Variable(s, _) => {
                    result.insert(s.to_string());
                }
                Capture::Subquery(t) => {
                    let sub_vars = t.variables();
                    result.extend(sub_vars);
                }
                _ => (),
            }
        }

        for neg in &self.negations {
            result.extend(neg.qt.variables())
        }

        result
    }

    /// Return all identifiers (function, variable and types) used in a query.
    /// This can be used to filter inputs without doing a full parse.
    pub fn identifiers(&self) -> Vec<String> {
        let mut result = Vec::new();
        for c in &self.captures {
            match c {
                Capture::Check(s) => result.push(s.to_string()),
                Capture::Subquery(t) => {
                    let mut sub_identifiers = t.identifiers();
                    result.append(&mut sub_identifiers);
                }
                _ => (),
            }
        }

        result
    }

    // Find all matches for the input described by the AST `root` node and its source code.
    // This is a simple wrapper around QueryTree::match_internal
    pub fn matches(&self, root: Node, source: &str) -> Vec<QueryResult> {
        let mut cache: Cache = FxHashMap::default();

        let mut results = self.match_internal(root, source, &mut cache);
        results.dedup();
        results
    }

    /// This is the core method for query matching.
    /// We start with outermost query and use tree-sitter's API to find all matching nodes.
    //  Due to our query predicates, this already takes care of all identifiers and variables.
    //  Once we have a match, we still need to recursively execute all subqueries and merge
    //  their results. Merging will remove results where a subquery requires different
    //  variable assignment from the rest.
    //  To avoid repeated work, we memoize results of subqueries in the `cache` hashmap and
    //  use them when feasible.
    //  TODO: Benchmark if caching or earlier variable enforcement is faster.
    fn match_internal(&self, root: Node, source: &str, cache: &mut Cache) -> Vec<QueryResult> {
        let mut qc = tree_sitter::QueryCursor::new();

        let num_patterns = self.query.pattern_count();
        let mut pattern_results = Vec::with_capacity(num_patterns + 1);
        for _ in 0..num_patterns {
            pattern_results.push(Vec::new());
        }

        for m in qc.matches(&self.query, root, source.as_bytes()) {
            // Process the query match, run subqueries and store the final QueryResults in pattern_results
            pattern_results[m.pattern_index].extend(self.process_match(cache, source, &m));
        }

        // Return an empty result if any of our patterns have 0 results.
        let have_failed_pattern = pattern_results.iter().any(|pr| pr.is_empty());
        if have_failed_pattern {
            return vec![];
        }

        // Try to merge the results of all patterns. If this fails we return an empty result
        let mut merged_results = Vec::new();
        for pr in pattern_results {
            if merged_results.is_empty() {
                merged_results.extend(pr)
            } else {
                merged_results = QueryTree::merge_query_results(&merged_results, &pr, source, true);
                if merged_results.is_empty() {
                    return merged_results;
                }
            }
        }

        // Enforce negative sub queries.
        merged_results
            .into_iter()
            .filter(|result| {
                let negative_query_matched = self.negations.iter().any(|neg| {
                    // run the negative sub query
                    let negative_results = neg.qt.match_internal(root, source, cache);

                    // check if any of its result are a valid match.
                    negative_results.into_iter().any(|n| {
                        // check if the negative match `m` is consistent with our result
                        if n.merge(result, source, false).is_none() {
                            return false;
                        }

                        // we have a match for the negative sub query, but we still need to enforce ordering.
                        // We know that the negative match has to come _after_ the node captured by the index
                        // previous_capture_index and _before_ the capture after that.
                        let index = neg.previous_capture_index;
                        if let Some(c) = result.get_capture_result(self.id, index as u32) {
                            // negative match is too early. skip it
                            if n.start_offset() < c.range.end {
                                return false;
                            }
                        };
                        if let Some(c) = result.get_capture_result(self.id, (index + 1) as u32) {
                            // negative match comes too late. skip it
                            if n.start_offset() > c.range.start {
                                return false;
                            }
                        }

                        true
                    })
                });

                !negative_query_matched
            })
            .collect()
    }

    // Process a single tree-sitter match and return all query results
    // This function is responsible for running all subqueries,
    // and veriyfing that negations don't match.
    fn process_match(
        &self,
        cache: &mut Cache,
        source: &str,
        m: &tree_sitter::QueryMatch,
    ) -> Vec<QueryResult> {
        let mut r = Vec::with_capacity(m.captures.len());
        let mut vars: FxHashMap<String, usize> =
            FxHashMap::with_capacity_and_hasher(self.variables.len(), Default::default());

        let mut subqueries = Vec::new();

        for c in m.captures {
            let capture = &self.captures[c.index as usize];

            let capture_result = CaptureResult {
                range: c.node.byte_range(),
                query_id: self.id,
                capture_idx: c.index,
            };

            // TODO: Do we need to store sub queries in captures as well?
            if !matches!(capture, Capture::Subquery(_)) {
                r.push(capture_result)
            }

            match capture {
                Capture::Variable(s, regex_constraint) => {
                    if let Some((negative, regex)) = regex_constraint {
                        let m = regex.is_match(&source[c.node.byte_range()]);
                        if (m && *negative) || (!m && !*negative) {
                            return vec![];
                        }
                    }
                    vars.insert(s.clone(), r.len() - 1);
                }
                Capture::Subquery(t) => {
                    subqueries.push((t, c));
                }
                Capture::Number(i) => {
                    if let Some(y) = parse_number_literal(&source[c.node.byte_range()]) {
                        if *i != y {
                            return vec![];
                        }
                    } else {
                        return vec![];
                    }
                }
                _ => (),
            }
        }

        let function = if let Some(c) = r.first() {
            c.range.clone()
        } else {
            0usize..0usize
        };

        let qr = QueryResult::new(r, vars, function);

        let query_results = subqueries.iter().fold(vec![qr], |results, (t, c)| {
            // avoid running subqueries if merging failed.
            if results.is_empty() {
                return results;
            }

            let key = CacheKey {
                query_id: t.id,
                node_id: c.node.id(),
            };

            // can't use entry API because match_internal requires another mutable reference to `cache`
            let sub_results = match cache.get(&key) {
                None => {
                    let v = t.match_internal(c.node, source, cache);
                    cache.insert(key.clone(), v);
                    cache.get(&key).unwrap()
                }
                Some(r) => r,
            };
            QueryTree::merge_query_results(&results, sub_results, source, false)
        });

        query_results
    }

    // Try to merge all matches of a subquery (`sub_results`) into matches of the main query `results`.
    // This enforces that variable assignments are coherent and can optionally enforce ordering
    // so that nodes captured by the subquery have to come after nodes that are already stored in the result.
    // This is used by multi pattern / compound queries to make sure `a` comes after `b` for the query `{a; b;}`
    fn merge_query_results(
        results: &[QueryResult],
        sub_results: &[QueryResult],
        source: &str,
        enforce_ordering: bool,
    ) -> Vec<QueryResult> {
        results
            .iter()
            .flat_map(move |r| {
                sub_results
                    .iter()
                    .filter_map(move |s| r.merge(s, source, enforce_ordering))
            })
            .collect()
    }
}
