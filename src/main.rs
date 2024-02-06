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

extern crate clap;
#[macro_use]
extern crate log;
extern crate rayon;
extern crate simplelog;
extern crate walkdir;

use colored::Colorize;
use rayon::iter::ParallelBridge;
use rayon::prelude::*;
use regex::Regex;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc};
use std::{collections::HashMap, path::Path};
use std::{collections::HashSet, fs};
use std::{io::prelude::*, path::PathBuf};
use thread_local::ThreadLocal;
use tree_sitter::Tree;
use walkdir::WalkDir;

use weggli::parse_search_pattern;
use weggli::query::QueryTree;
use weggli::result::QueryResult;
use weggli::RegexMap;

use cli::PATH_DASH_FOR_STDIN;

mod cli;

fn main() {
    reset_signal_pipe_handler();

    let args = cli::parse_arguments();

    if args.force_color {
        colored::control::set_override(true)
    }

    // Keep track of all variables used in the input pattern(s)
    let mut variables = HashSet::new();

    // Validate all regular expressions
    let regex_constraints = process_regexes(&args.regexes).unwrap_or_else(|e| {
        let msg = match e {
            RegexError::InvalidArg(s) => format!(
                "'{}' is not a valid argument of the form var=regex",
                s.red()
            ),
            RegexError::InvalidRegex(s) => format!("Regex error {}", s),
        };
        eprintln!("{}", msg);
        std::process::exit(1)
    });

    // Normalize all patterns and translate them into QueryTrees
    // We also extract the identifiers at this point
    // to use them for file filtering later on.
    // Invalid patterns trigger a process exit in validate_query so
    // after this point we know that all patterns are valid.
    // The loop also fills the `variables` set with used variable names.
    let work: Vec<WorkItem> = args
        .pattern
        .iter()
        .map(|pattern| {
            match parse_search_pattern(
                pattern,
                args.cpp,
                args.force_query,
                Some(regex_constraints.clone()),
            ) {
                Ok(qt) => {
                    let identifiers = qt.identifiers();
                    variables.extend(qt.variables());
                    WorkItem { qt, identifiers }
                }
                Err(qe) => {
                    eprintln!("{}", qe.message);
                    if !args.cpp
                        && parse_search_pattern(
                            pattern,
                            true,
                            args.force_query,
                            Some(regex_constraints.clone()),
                        )
                        .is_ok()
                    {
                        eprintln!("{} This query is valid in C++ mode (-X)", "Note:".bold());
                    }
                    std::process::exit(1);
                }
            }
        })
        .collect();

    for v in regex_constraints.variables() {
        if !variables.contains(v) {
            eprintln!("'{}' is not a valid query variable", v.red());
            std::process::exit(1)
        }
    }

    // Verify that the --include and --exclude regexes are valid.
    let helper_regex = |v: &[String]| -> Vec<Regex> {
        v.iter()
            .map(|s| {
                let r = Regex::new(s);
                r.unwrap_or_else(|e| {
                    eprintln!("Regex error {}", e);
                    std::process::exit(1)
                })
            })
            .collect()
    };

    let exclude_re = helper_regex(&args.exclude);
    let include_re = helper_regex(&args.include);

    // Collect files from input path(s) and/or stdin.
    let mut files: Vec<PathBuf> = Vec::new();
    args.paths.iter().for_each(|path| {
        if path == Path::new(PATH_DASH_FOR_STDIN) {
            std::io::stdin()
                .lock()
                .lines()
                .map_while(Result::ok)
                .map(|s| Path::new(&s).to_path_buf())
                .for_each(|p| files.push(p));
        } else {
            iter_files(path, args.extensions.clone())
                .map(|d| d.into_path())
                .for_each(|p| files.push(p));
        }
    });

    // Filter our input file set.
    if !exclude_re.is_empty() || !include_re.is_empty() {
        // Filter files based on include and exclude regexes
        files.retain(|f| {
            if exclude_re.iter().any(|r| r.is_match(&f.to_string_lossy())) {
                return false;
            }
            if include_re.is_empty() {
                return true;
            }
            include_re.iter().any(|r| r.is_match(&f.to_string_lossy()))
        });
    }

    info!("parsing {} files", files.len());
    if files.is_empty() {
        eprintln!("{}", String::from("No files to parse. Exiting...").red());
        std::process::exit(1)
    }

    // The main parallelized work pipeline
    rayon::scope(|s| {
        // spin up channels for worker communication
        let (ast_tx, ast_rx) = mpsc::channel();
        let (results_tx, results_rx) = mpsc::channel();

        // avoid lifetime issues
        let cpp = args.cpp;
        let w = &work;
        let before = args.before;
        let after = args.after;
        let enable_line_numbers = args.enable_line_numbers;

        // Spawn worker to iterate through files, parse potential matches and forward ASTs
        s.spawn(move |_| parse_files_worker(files, ast_tx, w, cpp));

        // Run search queries on ASTs and apply CLI constraints
        // on the results. For single query executions, we can
        // directly print any remaining matches. For multi
        // query runs we forward them to our next worker function
        s.spawn(move |_| execute_queries_worker(ast_rx, results_tx, w, &args));

        if w.len() > 1 {
            s.spawn(move |_| {
                multi_query_worker(results_rx, w.len(), before, after, enable_line_numbers)
            });
        }
    });
}

enum RegexError {
    InvalidArg(String),
    InvalidRegex(regex::Error),
}

impl From<regex::Error> for RegexError {
    fn from(err: regex::Error) -> RegexError {
        RegexError::InvalidRegex(err)
    }
}

/// Validate all passed regexes and compile them.
/// Returns an error if an invalid regex is supplied otherwise return a RegexMap
fn process_regexes(regexes: &[String]) -> Result<RegexMap, RegexError> {
    let mut result = HashMap::new();

    for r in regexes {
        let mut s = r.splitn(2, '=');
        let var = s.next().ok_or_else(|| RegexError::InvalidArg(r.clone()))?;
        let raw_regex = s.next().ok_or_else(|| RegexError::InvalidArg(r.clone()))?;

        let mut normalized_var = if var.starts_with('$') {
            var.to_string()
        } else {
            "$".to_string() + var
        };
        let negative = normalized_var.ends_with('!');

        if negative {
            normalized_var.pop(); // remove !
        }

        let regex = Regex::new(raw_regex)?;
        result.insert(normalized_var, (negative, regex));
    }
    Ok(RegexMap::new(result))
}

/// Recursively iterate through all files under `path` that match an ending listed in `extensions`
fn iter_files(path: &Path, extensions: Vec<String>) -> impl Iterator<Item = walkdir::DirEntry> {
    let is_hidden = |entry: &walkdir::DirEntry| {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    };

    WalkDir::new(path)
        .into_iter()
        .filter_entry(move |e| !is_hidden(e))
        .filter_map(|e| e.ok())
        .filter(move |entry| {
            if entry.file_type().is_dir() {
                return false;
            }

            let path = entry.path();

            match path.extension() {
                None => return false,
                Some(ext) => {
                    let s = ext.to_str().unwrap_or_default();
                    if !extensions.contains(&s.to_string()) {
                        return false;
                    }
                }
            }
            true
        })
}

struct WorkItem {
    qt: QueryTree,
    identifiers: Vec<String>,
}

/// Iterate over all paths in `files`, parse files that might contain a match for any of the queries
/// in `work` and send them to the next worker using `sender`.
fn parse_files_worker(
    files: Vec<PathBuf>,
    sender: Sender<(Arc<String>, Tree, String)>,
    work: &[WorkItem],
    is_cpp: bool,
) {
    let tl = ThreadLocal::new();

    files
        .into_par_iter()
        .for_each_with(sender, move |sender, path| {
            let maybe_parse = |path| {
                let c = match fs::read(path) {
                    Ok(content) => content,
                    Err(_) => return None,
                };

                let source = String::from_utf8_lossy(&c);

                let potential_match = work.iter().any(|WorkItem { qt: _, identifiers }| {
                    identifiers.iter().all(|i| source.find(i).is_some())
                });

                if !potential_match {
                    None
                } else {
                    let mut parser = tl
                        .get_or(|| RefCell::new(weggli::get_parser(is_cpp)))
                        .borrow_mut();
                    let tree = parser.parse(source.as_bytes(), None).unwrap();
                    Some((tree, source.to_string()))
                }
            };
            if let Some((source_tree, source)) = maybe_parse(&path) {
                sender
                    .send((
                        std::sync::Arc::new(source),
                        source_tree,
                        path.display().to_string(),
                    ))
                    .unwrap();
            }
        });
}

struct ResultsCtx {
    query_index: usize,
    path: String,
    source: std::sync::Arc<String>,
    result: weggli::result::QueryResult,
}

/// Fetches parsed ASTs from `receiver`, runs all queries in `work` on them and
/// filters the results based on the provided regex `constraints` and --unique --limit switches.
/// For single query runs, the remaining results are directly printed. Otherwise they get forwarded
/// to `multi_query_worker` through the `results_tx` channel.
fn execute_queries_worker(
    receiver: Receiver<(Arc<String>, Tree, String)>,
    results_tx: Sender<ResultsCtx>,
    work: &[WorkItem],
    args: &cli::Args,
) {
    receiver.into_iter().par_bridge().for_each_with(
        results_tx,
        |results_tx, (source, tree, path)| {
            // For each query
            work.iter()
                .enumerate()
                .for_each(|(i, WorkItem { qt, identifiers: _ })| {
                    // Run query
                    let matches = qt.matches(tree.root_node(), &source);

                    if matches.is_empty() {
                        return;
                    }

                    // Enforce --unique
                    let check_unique = |m: &QueryResult| {
                        if args.unique {
                            let mut seen = HashSet::new();
                            m.vars
                                .keys()
                                .map(|k| m.value(k, &source).unwrap())
                                .all(|x| seen.insert(x))
                        } else {
                            true
                        }
                    };

                    let mut skip_set = HashSet::new();

                    // Enforce --limit
                    let check_limit = |m: &QueryResult| {
                        if args.limit {
                            skip_set.insert(m.start_offset())
                        } else {
                            true
                        }
                    };

                    // Print match or forward it if we are in a multi query context
                    let process_match = |m: QueryResult| {
                        // single query
                        if work.len() == 1 {
                            let line = source[..m.start_offset()].matches('\n').count() + 1;
                            println!(
                                "{}:{}\n{}",
                                path.clone().bold(),
                                line,
                                m.display(
                                    &source,
                                    args.before,
                                    args.after,
                                    args.enable_line_numbers
                                )
                            );
                        } else {
                            results_tx
                                .send(ResultsCtx {
                                    query_index: i,
                                    result: m,
                                    path: path.clone(),
                                    source: source.clone(),
                                })
                                .unwrap();
                        }
                    };

                    matches
                        .into_iter()
                        .filter(check_unique)
                        .filter(check_limit)
                        .for_each(process_match);
                });
        },
    );
}

/// For multi query runs, we collect all independent results first and filter
/// them to make sure that variable assignments are valid for all queries.
fn multi_query_worker(
    results_rx: Receiver<ResultsCtx>,
    num_queries: usize,
    before: usize,
    after: usize,
    enable_line_numbers: bool,
) {
    let mut query_results = Vec::with_capacity(num_queries);
    for _ in 0..num_queries {
        query_results.push(Vec::new());
    }

    // collect all results
    for ctx in results_rx {
        query_results[ctx.query_index].push(ctx);
    }

    // filter results.
    // We now have a list of results for each query in query_results, but we still need to ensure
    // that we only show results for query A that can be combined with at least one result in query B
    // (and C and D).
    // TODO: The runtime of this approach is pretty terrible, think about improving it.
    let filter = |x: &mut Vec<ResultsCtx>, y: &mut Vec<ResultsCtx>| {
        x.retain(|r| {
            y.iter()
                .any(|f| r.result.chainable(&r.source, &f.result, &f.source))
        })
    };

    for i in 0..query_results.len() {
        let (part1, part2) = query_results.split_at_mut(i + 1);
        let a = part1.last_mut().unwrap();
        for b in part2 {
            filter(a, b);
            filter(b, a);
        }
    }

    // Print remaining results
    query_results.into_iter().for_each(|rv| {
        rv.into_iter().for_each(|r| {
            let line = r.source[..r.result.start_offset()].matches('\n').count() + 1;
            println!(
                "{}:{}\n{}",
                r.path.bold(),
                line,
                r.result
                    .display(&r.source, before, after, enable_line_numbers)
            );
        })
    });
}

// Exit on SIGPIPE
// see https://github.com/rust-lang/rust/issues/46016#issuecomment-605624865
fn reset_signal_pipe_handler() {
    #[cfg(target_family = "unix")]
    {
        use nix::sys::signal;

        unsafe {
            let _ = signal::signal(signal::Signal::SIGPIPE, signal::SigHandler::SigDfl)
                .map_err(|e| eprintln!("{}", e));
        }
    }
}
