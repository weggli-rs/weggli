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
use rustc_hash::FxHashMap;
use std::ops::Range;

/// Struct for storing (partial) query matches.
/// We really don't want to keep track of tree-sitter AST lifetimes so
/// we do not store full nodes, but only their source range.
/// TODO: Improve this struct + benchmarking
#[derive(Debug, Eq, PartialEq)]
pub struct QueryResult {
    // for each captured node we store the offset ranges of its src location
    pub captures: Vec<CaptureResult>,
    // Mapping from Variables to index in `captures`
    pub vars: FxHashMap<String, usize>,
    // Range of the outermost node. This is badly named as it does not have to be a
    // function definition, but for final query results it normally is.
    function: std::ops::Range<usize>,
}

/// Stores the result (== source range) for a single capture.
/// We also store the corresponding query id and capture index
/// to make it possible to look up the result for a certain capture
/// index (see QueryResult::get_capture_result)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureResult {
    pub range: std::ops::Range<usize>,
    pub query_id: usize,
    pub capture_idx: u32,
}

impl<'b> QueryResult {
    pub fn new(
        captures: Vec<CaptureResult>,
        vars: FxHashMap<String, usize>,
        function: std::ops::Range<usize>,
    ) -> QueryResult {
        QueryResult {
            captures,
            vars,
            function,
        }
    }

    pub fn start_offset(&self) -> usize {
        self.function.start
    }

    /// Returns a colored String representation of the result with `before` + `after`
    /// context lines around each captured node.
    pub fn display(
        &self,
        source: &'b str,
        before: usize,
        after: usize,
        enable_line_numbers: bool,
    ) -> String {
        let mut d = DisplayHelper::new(source);

        // add header
        d.add(self.function.start..self.function.start + 1);

        let mut sorted = self.captures.clone();
        sorted.sort_by(|a, b| a.range.start.cmp(&b.range.start));

        // Before printing out the different nodes, we first filter out overlapping nodes.
        // If we matched on `(a + b)` and also captured `b` clean_ranges will not contain
        // the range for `b`.
        let mut clean_ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(self.captures.len());
        for r in sorted.into_iter().skip(1).map(|c| c.range) {
            if !clean_ranges.is_empty() && clean_ranges.last().unwrap().contains(&r.start) {
                continue;
            }
            clean_ranges.push(r.clone());
        }

        // Add highlighted elements
        for r in clean_ranges.into_iter() {
            d.highlight(r);
        }

        // add function ending
        d.add(self.function.end - 1..self.function.end);

        d.display(before, after, enable_line_numbers)
    }

    /// Return the captured value for a variable.
    pub fn value(&self, var: &str, source: &'b str) -> Option<&'b str> {
        match self.vars.get(var) {
            None => None,
            Some(i) => Some(&source[self.captures[*i].range.clone()]),
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

        let mut captures = self.captures.clone();

        if enforce_order
            && other
                .captures
                .iter()
                .any(|r| self.captures.iter().any(|r2| r.range.start <= r2.range.end))
        {
            return None;
        }

        captures.extend(other.captures.clone());

        for (k, v) in other.vars.iter() {
            match self.value(k, source) {
                None => {
                    vars.insert(k.clone(), v + self.captures.len());
                }
                Some(s) => {
                    if s != other.value(k, source).unwrap() {
                        return None;
                    }
                }
            }
        }

        Some(QueryResult::new(captures, vars, self.function.clone()))
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

    /// Try to find the result for the capture `capture_idx` in query `query_id`
    pub fn get_capture_result(&self, query_id: usize, capture_idx: u32) -> Option<&CaptureResult> {
        self.captures
            .iter()
            .find(|c| c.capture_idx == capture_idx && c.query_id == query_id)
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
                .filter_map(move |s| r.merge(s, source, enforce_order))
        })
        .collect()
}

struct DisplayHelper<'a> {
    lines: Vec<(usize, &'a str, u8)>,
    highlights: Vec<Range<usize>>,
    curr: usize,
    first: usize,
    last: usize,
}

impl<'a> DisplayHelper<'a> {
    fn new(source: &'a str) -> DisplayHelper<'a> {
        let mut lines = Vec::new();
        let mut offset = 0;
        for l in source.split('\n') {
            lines.push((offset, l, 0));
            offset += l.len() + 1;
        }

        DisplayHelper {
            lines,
            highlights: Vec::new(),
            curr: 0,
            first: 0xFFFFFFFF,
            last: 0,
        }
    }

    fn highlight(&mut self, range: Range<usize>) {
        self.add(range.clone());
        self.highlights.push(range);
    }

    fn add(&mut self, range: Range<usize>) {
        for (line_nr, (offset, l, print)) in self.lines.iter_mut().enumerate().skip(self.curr) {
            if *print == 0 && range.start < (*offset + l.len()) && (*offset < range.end) {
                self.first = line_nr.min(self.first);
                self.last = line_nr.max(self.last);
                *print = 1;
            } else if *offset >= range.end {
                self.curr -= 1;
                break;
            }
            self.curr += 1;
        }
    }

    fn format(&self, start_offset: usize, l: &str, hindex: usize) -> String {
        let highlights =
            self.highlights.iter().skip(hindex).filter(|range| {
                range.start <= (start_offset + l.len()) && start_offset <= range.end
            });
        let mut result = String::new();

        let mut current_offset = 0;
        for h in highlights {
            let start = if h.start > start_offset {
                h.start - start_offset
            } else {
                0
            };

            let end = if h.end < start_offset + l.len() {
                h.end - start_offset
            } else {
                l.len()
            };

            result += &l[current_offset..start];
            result += &format!("{}", l[start..end].red());
            current_offset = end;
        }
        result += &l[current_offset..l.len()];
        result += "\n";
        result
    }

    fn display(&mut self, before: usize, after: usize, enable_line_numbers: bool) -> String {
        let mut result = String::new();
        let mut skipped = true;

        for i in self.first..self.last + 1 {
            if self.lines[i].2 != 1 {
                continue;
            }

            let b = if i >= before {
                self.first.max(i - before)
            } else {
                self.first
            };
            let a = self.last.min(i + after);

            for j in b..i {
                self.lines[j].2 = 2;
            }

            for j in i..(a + 1) {
                self.lines[j].2 = 2;
            }
        }

        for (line_nr, (offset, l, p)) in self.lines.iter().enumerate() {
            if *p == 0 {
                if !skipped {
                    skipped = true;
                    if enable_line_numbers {
                        let length = (line_nr - 1).to_string().len();
                        if length < 4 {
                            result += &" ".repeat(4 - length)
                        }
                        result += &".".repeat(length);
                        result += "\n"
                    } else {
                        result += "...\n"
                    }
                }
                continue;
            }

            if enable_line_numbers {
                result += &format!("{:>4}: ", line_nr + 1);
            }
            result += &self.format(*offset, l, 0);
            skipped = false;
        }

        let t = if skipped {
            6
        } else {
            1
        };

        result.truncate(result.len() - t);

        result
    }
}
