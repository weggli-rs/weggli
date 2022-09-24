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
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, EditMode};
use rustyline::config::OutputStreamType;

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc};
use std::{collections::HashMap, path::Path};
use std::{collections::HashSet, fs};
use std::convert::TryInto;
use std::{io::prelude::*, path::PathBuf};
use tree_sitter::Tree;
use walkdir::WalkDir;
use weggli::RegexMap;

use weggli::parse_search_pattern;
use weggli::query::QueryTree;
use weggli::result::QueryResult;
use weggli::Source;

#[cfg(feature="binja")]
use weggli::binja;

mod cli;
mod repl;

fn main() {
    reset_signal_pipe_handler();

    let args = cli::parse_arguments();

    if args.force_color {
        colored::control::set_override(true)
    }

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

    if args.repl {
        start_repl(args, regex_constraints);
    } else {
        if let Err(msg) = normal_mode(args, regex_constraints) {
            eprintln!("{}", msg);
            std::process::exit(1);
        }
    }
}

fn collect_files(args: &cli::Args) -> (Vec<(PathBuf, u64)>, u64) {
    // Verify that the --include and --exclude regexes are valid.
    let helper_regex = |v: &[String]| -> Vec<Regex> {
        v.iter()
            .map(|s| {
                let r = Regex::new(s);
                match r {
                    Ok(regex) => regex,
                    Err(e) => {
                        eprintln!("Regex error {}", e);
                        std::process::exit(1)
                    }
                }
            })
            .collect()
    };

    let exclude_re = helper_regex(&args.exclude);
    let include_re = helper_regex(&args.include);

    // Collect and filter our input file set.
    let mut files: Vec<(PathBuf, u64)> = if args.path.to_string_lossy() == "-" {
        std::io::stdin()
            .lock()
            .lines()
            .filter_map(|l| l.ok())
            .map(|s| {
                let path = Path::new(&s).to_path_buf();
                let sz = fs::metadata(&path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                (path, sz)
            })
            .collect()
    } else {
        iter_files(&args.path, args.extensions.clone())
            .map(|d| {
                let path = d.into_path();
                let sz = fs::metadata(&path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                (path, sz)
            })
            .collect()
    };

    let total_size = files.iter().map(|it| it.1).sum();

    if !exclude_re.is_empty() || !include_re.is_empty() {
        // Filter files based on include and exclude regexes
        files.retain(|f| {
            if exclude_re.iter().any(|r| r.is_match(&f.0.to_string_lossy())) {
                return false;
            }
            if include_re.is_empty() {
                return true;
            }
            include_re.iter().any(|r| r.is_match(&f.0.to_string_lossy()))
        });
    }

    (files, total_size)
}

fn normal_mode(args: cli::Args, regex_constraints: RegexMap) -> Result<(), String> {
    // Keep track of all variables used in the input pattern(s)
    let mut variables = HashSet::new();

    // Normalize all patterns and translate them into QueryTrees
    // We also extract the identifiers at this point
    // to use them for file filtering later on.
    // Invalid patterns trigger a process exit in validate_query so
    // after this point we now that all patterns are valid.
    // The loop also fills the `variables` set with used variable names.
    // Normalize all patterns and translate them into QueryTrees
    // We also extract the identifiers at this point
    // to use them for file filtering later on.
    // Invalid patterns trigger a process exit in validate_query so
    // after this point we now that all patterns are valid.
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
            std::process::exit(1);
        }
    }

    let (files, _) = collect_files(&args);

    info!("parsing {} files", files.len());
    if files.is_empty() {
        eprintln!("{}", String::from("No files to parse. Exiting...").red());
        std::process::exit(1)
    }

    #[cfg(feature="binja")]
    let binja = args.binja;

    #[cfg(feature="binja")]
    if binja {
        binaryninja::headless::init();
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

        // Spawn worker to iterate through files, parse potential matches and forward ASTs
        #[cfg(feature="binja")]
        if args.binja {
            s.spawn(move|_| parse_binja_binaries_worker(files, ast_tx, Some(w), None));
        } else {
            s.spawn(move |_| parse_files_worker(files, ast_tx, Some(w), None, cpp));
        }

        // Spawn worker to iterate through files, parse potential matches and forward ASTs
        #[cfg(not(feature="binja"))]
        s.spawn(move |_| parse_files_worker(files, ast_tx, Some(w), None, cpp));

        // Run search queries on ASTs and apply CLI constraints
        // on the results. For single query executions, we can
        // directly print any remaining matches. For multi
        // query runs we forward them to our next worker function
        s.spawn(move |_| execute_queries_worker(ast_rx, results_tx, w, &args));

        if w.len() > 1 {
            s.spawn(move |_| multi_query_worker(results_rx, w.len(), before, after));
        }
    });

    #[cfg(feature="binja")]
    if binja {
        binaryninja::headless::shutdown();
    }

    Ok(())
}

fn start_repl(args: cli::Args, regex_constraints: RegexMap) {
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .output_stream(OutputStreamType::Stdout)
        .build();

    let mut rl = rustyline::Editor::with_config(config);
    rl.set_helper(Some(repl::ReplHelper::new()));

    let (files, total_size) = collect_files(&args);

    let mut parsed = HashMap::new();

    #[cfg(feature="binja")]
    let binja = args.binja;

    #[cfg(feature="binja")]
    if binja {
        binaryninja::headless::init();
    }

    info!("parsing {} files", files.len());
    if files.is_empty() {
        eprintln!("{}", String::from("No files to parse. Exiting...").red());
        std::process::exit(1)
    }

    let progress_bar = indicatif::ProgressBar::new(total_size.try_into().unwrap());
    let style = indicatif::ProgressStyle::default_bar()
        .template("{prefix:.bold.dim} {spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})");
    progress_bar.set_style(style);
    progress_bar.set_prefix("Loading");

    let cpp = args.cpp;
    // Parallelized parsing pipeline
    rayon::scope(|s| {
        let (ast_tx, ast_rx) = mpsc::channel();

        // Spawn worker to iterate through files, parse potential matches and forward ASTs
        // We send a None WorkItem, as this is constant startup parsing overhead.
        #[cfg(feature="binja")]
        if args.binja {
            s.spawn(|_| parse_binja_binaries_worker(files, ast_tx, None, Some(&progress_bar)));
        } else {
            s.spawn(|_| parse_files_worker(files, ast_tx, None, Some(&progress_bar), cpp));
        }

        // Spawn worker to iterate through files, parse potential matches and forward ASTs
        #[cfg(not(feature="binja"))]
        s.spawn(|_| parse_files_worker(files, ast_tx, None, Some(&progress_bar), cpp));

        // Spawn another worker to gather the files into the hashmap
        s.spawn(|_| gather_parsed_worker(ast_rx, &mut parsed));
    });

    progress_bar.finish_and_clear();

    loop {
        let readline = rl.readline(&(">> ".red()));
        match readline {
            Ok(pattern) => {
                if let Err(msg) = do_repl_single_query(pattern, &args, &regex_constraints, &parsed)
                {
                    eprintln!("{}", msg.red().bold());
                }
            }
            Err(ReadlineError::Eof) => break,
            Err(_) => println!("{}", "No input".red()),
        };
    }

    #[cfg(feature="binja")]
    if binja {
        binaryninja::headless::shutdown();
    }
}

fn do_repl_single_query(
    pattern: String,
    args: &cli::Args,
    regex_constraints: &RegexMap,
    parsed: &HashMap<Location, (Tree, Arc<Box<dyn Source + Send + Sync>>)>,
) -> Result<(), String> {
    let mut variables = HashSet::new();

    let qt = parse_search_pattern(
        &pattern,
        args.cpp,
        args.force_query,
        Some(regex_constraints.clone()),
    )
    .map_err(|qe| {
        if !args.cpp
            && parse_search_pattern(
                &pattern,
                true,
                args.force_query,
                Some(regex_constraints.clone()),
            )
            .is_ok()
        {
            format!(
                "{}\n{} This query is valid in C++ mode (-X)",
                qe.message,
                "Note:".bold(),
            )
        } else {
            qe.message
        }
    })?;

    let identifiers = qt.identifiers();
    variables.extend(qt.variables());

    let work = WorkItem { qt, identifiers };

    for v in regex_constraints.variables() {
        if !variables.contains(v) {
            return Err(format!("'{}' is not a valid query variable", v.red()));
        }
    }

    rayon::scope(|s| {
        let (ast_tx, ast_rx) = mpsc::channel();
        let (results_tx, _results_rx) = mpsc::channel();

        s.spawn(move |_| retrieve_asts(ast_tx, parsed));

        let w = [work];
        // Run search queries on ASTs and apply CLI constraints
        // on the results. For single query executions, we can
        // directly print any remaining matches. For multi
        // query runs we forward them to our next worker function
        s.spawn(move |_| execute_queries_worker(ast_rx, results_tx, &w, args));

        // No need to spawn a multi_query_worker, as w.len() is always 1.
    });

    Ok(())
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

            if extensions.is_empty() {
                return true;
            }

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Location {
    SourceFile{ path: String },
    #[cfg(feature="binja")]
    BinaryFunction{ path: String, address: u64 },
}

impl Location {
    pub  fn format_with_line(&self, line: usize) -> String {
        match self {
            Location::SourceFile { path: p } => {
                format!("{}:{}", p.green().bold(), line )
            },
            #[cfg(feature="binja")]
            Location::BinaryFunction { path: p, address } => {
                let address = format!("0x{:x}", address);
                format!("{}: {}", p.green().bold(), address.yellow())
            },
        }
    }
}

/// Iterate over all paths in `files`, parse files that might contain a match for any of the queries
/// in `work` and send them to the next worker using `sender`.
fn parse_files_worker(
    files: Vec<(PathBuf, u64)>,
    sender: Sender<(Arc<Box<dyn Source + Send + Sync>>, Tree, Location)>,
    work: Option<&[WorkItem]>,
    progress_bar: Option<&indicatif::ProgressBar>,
    is_cpp: bool,
) {
    files
        .into_par_iter()
        .for_each_with(sender, move |sender, (path, sz)| {
            let maybe_parse = |path| {
                let c = match fs::read(path) {
                    Ok(content) => content,
                    Err(_) => return None,
                };

                let source = String::from_utf8_lossy(&c);

                // If we know what we're looking for, pre-filter during the parsing stage.
                if let Some(work) = work {
                    let potential_match = work.iter().any(|WorkItem { qt: _, identifiers }| {
                        identifiers.iter().all(|i| source.find(i).is_some())
                    });

                    if !potential_match {
                        None
                    } else {
                        Some((weggli::parse(&source, is_cpp), source.to_string()))
                    }
                }
                // We don't have a workitem, so we will parse the complete file. This is okay,
                // as we're probably in a situation where there is a one-time parsing
                // overhead in the repl.
                else {
                    Some((weggli::parse(&source, is_cpp), source.to_string()))
                }
            };
            if let Some((source_tree, source)) = maybe_parse(&path) {
                sender
                    .send((
                        std::sync::Arc::new(Box::new(source)),
                        source_tree,
                        Location::SourceFile{ path: path.display().to_string() },
                    ))
                    .unwrap();
                if let Some(progress) = progress_bar {
                    progress.inc(sz);
                }
            }
        });
}

#[cfg(feature="binja")]
fn parse_binja_binaries_worker(
    files: Vec<(PathBuf, u64)>,
    sender: Sender<(Arc<Box<dyn Source + Send + Sync>>, Tree, Location)>,
    _work: Option<&[WorkItem]>,
    progress_bar: Option<&indicatif::ProgressBar>,
) {
    files
        .into_par_iter()
        .for_each_with(sender, move |sender, (path, sz)| {
            let decomp = binja::Decompiler::from_file(&path);

            let n_functions = decomp.functions().len();
            let chunk_size: f64 = sz as f64 / n_functions as f64;

            for (i, function) in decomp.functions().iter().enumerate() {
                let decompiled = decomp.decompile_function(&function);
                let source_tree = weggli::parse(decompiled.text(), false);
                sender
                    .send((
                        std::sync::Arc::new(Box::new(decompiled)),
                        source_tree,
                        Location::BinaryFunction{ path: path.display().to_string(), address: function.start() },
                    ))
                    .unwrap();
                if let Some(progress) = progress_bar {
                    let inc = ((i+1) as f64 * chunk_size) as u64 - (i as f64 * chunk_size) as u64;
                    progress.inc(inc as u64);
                }
            }
        });
}

struct ResultsCtx {
    query_index: usize,
    location: Location,
    source: std::sync::Arc<Box<dyn Source + Send + Sync>>,
    result: weggli::result::QueryResult,
}


/// Fetches parsed ASTs from `receiver`, runs all queries in `work` on them and
/// filters the results based on the provided regex `constraints` and --unique --limit switches.
/// For single query runs, the remaining results are directly printed. Otherwise they get forwarded
/// to `multi_query_worker` through the `results_tx` channel.
fn execute_queries_worker(
    receiver: Receiver<(Arc<Box<dyn Source + Send + Sync>>, Tree, Location)>,
    results_tx: Sender<ResultsCtx>,
    work: &[WorkItem],
    args: &cli::Args,
) {
    receiver.into_iter().par_bridge().for_each_with(
        results_tx,
        |results_tx, (source, tree, location)| {
            // For each query
            work.iter()
                .enumerate()
                .for_each(|(i, WorkItem { qt, identifiers: _ })| {
                    // Run query
                    let matches = qt.matches(tree.root_node(), source.text());

                    if matches.is_empty() {
                        return;
                    }

                    // Enforce --unique
                    let check_unique = |m: &QueryResult| {
                        if args.unique {
                            let mut seen = HashSet::new();
                            m.vars
                                .keys()
                                .map(|k| m.value(k, source.text()).unwrap())
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
                            let line = source.text()[..m.start_offset()].matches('\n').count() + 1;
                            println!(
                                "{}\n{}",
                                location.format_with_line(line),
                                m.display(source.as_ref(), args.before, args.after)
                            );
                        } else {
                            results_tx
                                .send(ResultsCtx {
                                    query_index: i,
                                    result: m,
                                    location: location.clone(),
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
                .any(|f| r.result.chainable(r.source.text(), &f.result, f.source.text()))
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
            let line = r.source.text()[..r.result.start_offset()].matches('\n').count() + 1;
            println!(
                "{}\n{}",
                r.location.format_with_line(line),
                r.result.display(r.source.as_ref(), before, after)
            );
        })
    });
}

fn gather_parsed_worker(
    receiver: Receiver<(Arc<Box<dyn Source + Send + Sync>>, Tree, Location)>,
    hashmap: &mut HashMap<Location, (Tree, Arc<Box<dyn Source + Send + Sync>>)>,
) {
    for (source, tree, location) in receiver.into_iter() {
        hashmap.insert(location, (tree, source));
    }
}

fn retrieve_asts(
    sender: Sender<(Arc<Box<dyn Source + Send + Sync>>, Tree, Location)>,
    hashmap: &HashMap<Location, (Tree, Arc<Box<dyn Source + Send + Sync>>)>,
) {
    for (location, (tree, source)) in hashmap {
        sender
            .send((source.clone(), tree.clone(), location.clone()))
            .unwrap();
    }
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
