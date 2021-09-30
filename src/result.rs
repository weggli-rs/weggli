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

use colored::Colorize;
use std::{cmp, collections::HashMap};

/// Struct for storing (partial) query matches.
/// We really don't want to keep track of tree-sitter AST lifetimes so
/// we do not store full nodes, but only their source range.
/// TODO: Improve this struct + benchmarking

#[derive(Debug)]
pub struct QueryResult {
    // for each captured node we store the offset ranges of its src location
    ranges: Vec<std::ops::Range<usize>>,
    // Mapping from Variables to index in `ranges`
    pub vars: HashMap<String, usize>,
    // Range of the outermost node. This is badly named as it does not have to be a
    // function definition, but for final query results it normally is.
    function: std::ops::Range<usize>,
}

impl<'a, 'b> QueryResult {
    pub fn new(
        mut ranges: Vec<std::ops::Range<usize>>,
        vars: HashMap<String, usize>,
        function: std::ops::Range<usize>,
    ) -> QueryResult {
        ranges.sort_by(|a, b| a.start.cmp(&b.start));

        QueryResult {
            ranges,
            vars,
            function,
        }
    }

    pub fn start_offset(&self) -> usize {
        self.function.start
    }

    /// Returns a colored String representation of the result with `before` + `after`
    /// context lines around each captured node.
    pub fn display(&self, source: &'b str, before: usize, after: usize) -> String {
        let mut result = String::new();

        // Add two lines of the function header
        // TODO: We should just store the range of the header and always print it in full.
        let mut header_end = linebreak_index(source, self.function.start, 1, false);

        if self.ranges.len() > 1 {
            // Ensure we don't overlap with the range of the next node.
            header_end = cmp::min(header_end, self.ranges[1].start - 1);
        }

        result += &source[self.function.start..header_end];

        let mut offset = header_end;

        // Before printing out the different nodes, we first filter out overlapping nodes.
        // If we matched on `(a + b)` and also captured `b` clean_ranges will not contain
        // the range for `b`.
        let mut clean_ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(self.ranges.len());
        for r in self.ranges.iter().skip(1) {
            if !clean_ranges.is_empty() && clean_ranges.last().unwrap().contains(&r.start) {
                continue;
            }
            clean_ranges.push(r.clone());
        }

        // Iterate over all remaining nodes and print them
        for (index, r) in clean_ranges.iter().enumerate() {
            if r.start <= offset {
                continue;
            }

            // Print lines before/after the match, based on -A / -B
            let start = linebreak_index(source, r.start, before, true);
            let mut end = linebreak_index(source, r.end, after, false);

            // Avoid overlapping with the next node
            if index < clean_ranges.len() - 1 && r.end < clean_ranges[index + 1].start {
                end = cmp::min(end, clean_ranges[index + 1].start - 1);
            }

            // Never go beyond the function boundary.
            end = cmp::min(end, self.function.end);

            if start <= offset {
                // we are not skipping anything
                result += &source[offset..r.start];
            } else {
                // indicate that some code is skipped
                result += "..";
                result += &source[start..r.start];
            }
            // Mark the node itself in red.
            result += &format!("{}", &source[r.start..r.end].red());
            result += &source[r.end..end];
            offset = end;
        }

        // Print function ending.
        if offset < self.function.end {
            let last_line = linebreak_index(source, self.function.end, 0, true);
            result += "..";
            result += &source[last_line..self.function.end];
        }

        result
    }

    /// Return the captured value for a variable.
    pub fn value(&self, var: &str, source: &'b str) -> Option<&'b str> {
        match self.vars.get(var) {
            None => None,
            Some(i) => Some(&source[self.ranges[*i].clone()]),
        }
    }

    /// Try to merge two QueryResults from the same source file.
    /// The function returns None if the variable assignments for the two results differ.
    /// If `enforce_order` is set this can fail because the new ranges
    /// are not strictly after the current ranges. 
    pub fn merge(
        &self,
        other: &QueryResult,
        source: &str,
        enforce_order: bool,
    ) -> Option<QueryResult> {
        let mut vars = self.vars.clone();

        let mut ranges = self.ranges.clone();

        if enforce_order {
            if other
                .ranges
                .iter()
                .any(|r| self.ranges.iter().any(|r2| r.start <= r2.end))
            {
                return None;
            }
        }

        ranges.extend(other.ranges.clone());

        for (k, v) in other.vars.iter() {
            match self.value(k, source) {
                None => {
                    vars.insert(k.clone(), v + self.ranges.len());
                }
                Some(s) => {
                    if s != other.value(k, source).unwrap() {
                        return None;
                    }
                }
            }
        }

        Some(QueryResult::new(ranges, vars, self.function.clone()))
    }

    /// Checks if two QueryResults from different source files have compatible variable assignments
    pub fn chainable(&self, source: &str, other: &QueryResult, other_source: &str) -> bool {
        !other.vars.iter().any(|(k, _)| {
            if let Some(value) = self.value(k, source) {
                value != other.value(k, other_source).unwrap()
            } else {
                false
            }
        })
    }
}

// Try to merge sub_results into each result.
pub fn merge_results(
    results: &[QueryResult],
    sub_results: &[QueryResult],
    source: &str,
    enforce_order: bool,
) -> Vec<QueryResult> {
    results
        .iter()
        .flat_map(|r| {
            sub_results
                .iter()
                .filter_map(move |s| r.merge(&s, source, enforce_order))
        })
        .collect()
}


// Returns the index of the nth newline before (if `backwards` is set ) or after `source[index]`
// This is used to display additional context around captured nodes. If not enough newlines
// exist the function will return 0 (for backwards) or source.len().
fn linebreak_index(source: &str, index: usize, count: usize, backwards: bool) -> usize {
    let length = source.len();

    let mut f;
    let mut b;

    let iter: &mut dyn Iterator<Item = (usize, char)> = if !backwards {
        f = source[index..length].char_indices();
        &mut f
    } else {
        b = source[..index].char_indices().rev();
        &mut b
    };

    let newline_index = iter.filter(|(_, c)| *c == '\n').nth(count);

    match newline_index {
        Some((i, _)) if !backwards => cmp::min(length, index + i + 1),
        Some((i, _)) => i,
        None if !backwards => length,
        None => 0,
    }
}

#[test]
fn test_linebreak_index() {
    let input = "aaa\nbbb\nccc\nd";
    let index = input.find('b').unwrap();

    assert_eq!(linebreak_index(&input, index, 1, true), 0);
    assert_eq!(linebreak_index(&input, index, 1, false), input.find('d').unwrap());
    assert_eq!(linebreak_index(&input, index, 5, false), input.len());
    assert_eq!(linebreak_index(&input, index, 4, true), 0);
}
