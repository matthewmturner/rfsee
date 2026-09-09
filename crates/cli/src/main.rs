use std::{
    fs::File,
    io::{self, IsTerminal},
    num::NonZeroUsize,
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
    time::{Instant, SystemTime},
};

use clap::{ArgAction, Parser, Subcommand};
use rfsee_tf_idf::{
    error::{RFSeeError, RFSeeResult},
    get_index_path, search_index, Index, RfcSearchResult, Runtime, TfIdf,
};

mod browser;
mod config;
mod format;
mod inline;

use format::{format_log_line, format_score, format_search_results_tsv, parse_score};

#[derive(Clone, Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// Increase logging detail (-v, -vv, -vvv)
    #[arg(short = 'v', long, action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Number of worker threads available to the runtime. Defaults to the available parallelism
    /// of the machine.
    #[arg(long, global = true, default_value_t = Runtime::available_parallelism())]
    parallelism: NonZeroUsize,

    /// Path to the configuration file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Debug, Subcommand)]
enum Command {
    Index {
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    Search {
        /// Print results as TSV without the interactive inline picker.
        #[arg(long)]
        plain: bool,
        #[arg(short, long)]
        terms: String,
        #[arg(short, long)]
        index_path: Option<PathBuf>,
        /// Only show results with a score greater than this decimal value (default: 0.001).
        #[arg(long, value_parser = parse_score)]
        min_score: Option<i32>,
    },
}

static VERBOSITY: AtomicU8 = AtomicU8::new(0);

fn log(level: u8, msg: impl std::fmt::Display) {
    if VERBOSITY.load(Ordering::Relaxed) >= level {
        eprintln!("{}", format_log_line(SystemTime::now(), msg));
    }
}

fn log_progress(message: &str) {
    log(2, message);
}

fn filter_results_by_score(results: &mut Vec<RfcSearchResult>, min_score: i32) {
    results.retain(|result| result.score > min_score);
}

fn handle_command(args: Args, runtime: &Runtime) -> RFSeeResult<()> {
    let config_path = args.config.clone();
    if let Some(command) = args.command {
        match command {
            Command::Index { path } => {
                log(1, "Loading RFCs");
                let start = Instant::now();
                let mut index = TfIdf::default();
                let report = index.par_load_rfcs_with_report(runtime, log_progress)?;
                log(2, format!("Loaded RFCs in {:?}", start.elapsed()));
                log(
                    2,
                    format!(
                        "RFCs: {} loaded, {} skipped, {} total",
                        report.loaded.len(),
                        report.failures.len(),
                        report.total
                    ),
                );
                if args.verbose >= 3 {
                    for url in &report.loaded {
                        log(3, format!("Fetched {url}"));
                    }
                    for failure in &report.failures {
                        log(
                            3,
                            format!("Skipped RFC {}: {}", failure.rfc, failure.reason),
                        );
                    }
                }

                log(1, "Building index");
                let building_index_start = Instant::now();
                index.finish(log_progress);
                log(
                    2,
                    format!("Built index in {:?}", building_index_start.elapsed()),
                );
                log(
                    2,
                    format!(
                        "Index: {} documents, {} terms",
                        index.index.rfc_details.len(),
                        index.index.term_scores.len()
                    ),
                );

                let saving_start = Instant::now();
                let index_path = get_index_path(path)?;
                log(1, "Saving index");
                log(2, format!("Index path: {}", index_path.display()));
                index.save(&index_path);
                log(2, format!("Saved index in {:?}", saving_start.elapsed()));
            }
            Command::Search {
                terms,
                index_path,
                plain,
                min_score,
            } => {
                log(1, "Loading index");
                let start = Instant::now();
                let index_path = get_index_path(index_path)?;
                log(2, format!("Index path: {}", index_path.display()));
                let file =
                    File::open(&index_path).map_err(|e| RFSeeError::IOError(e.to_string()))?;
                let index: Index = simd_json::from_reader(file)
                    .map_err(|e| RFSeeError::ParseError(e.to_string()))?;
                log(2, format!("Loaded index in {:?}", start.elapsed()));

                if args.verbose >= 3 {
                    for term in terms.split_whitespace() {
                        let matches = index.term_scores.get(term).map_or(0, |scores| scores.len());
                        log(3, format!("Term {term:?}: {matches} matching RFCs"));
                    }
                }

                log(1, "Searching index");
                let search_start = Instant::now();
                let mut results = search_index(terms, index);
                let search_execution_time = search_start.elapsed();
                log(2, format!("Search completed in {search_execution_time:?}"));
                let file_config = config::load(config_path.as_deref())?;
                let min_score = file_config.resolve_min_score(min_score);
                let unfiltered_count = results.len();
                filter_results_by_score(&mut results, min_score);
                log(
                    2,
                    format!(
                        "Results above {}: {} of {unfiltered_count}",
                        format_score(min_score),
                        results.len()
                    ),
                );
                if !plain
                    && io::stdin().is_terminal()
                    && io::stdout().is_terminal()
                    && !results.is_empty()
                {
                    if let Some(selected) =
                        inline::pick(&results).map_err(|e| RFSeeError::IOError(e.to_string()))?
                    {
                        let url = &results[selected].url;
                        browser::open(url).map_err(|e| {
                            RFSeeError::IOError(format!("Could not open {url} in the browser: {e}"))
                        })?;
                    }
                } else {
                    println!("{}", format_search_results_tsv(&results));
                }
            }
        }
    }
    Ok(())
}

fn main() -> RFSeeResult<()> {
    let args = Args::parse();
    VERBOSITY.store(args.verbose, Ordering::Relaxed);

    // The runtime lives for the whole process: created here, dropped (and its workers joined)
    // when main returns.
    let runtime = Runtime::new(args.parallelism);
    log(2, format!("Runtime parallelism: {}", runtime.parallelism()));

    handle_command(args, &runtime)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use rfsee_tf_idf::RfcSearchResult;

    use super::{filter_results_by_score, Args, Command};

    #[test]
    fn parallelism_defaults_to_available_parallelism() {
        let args = Args::try_parse_from(["rfsee", "index"]).unwrap();
        assert_eq!(
            args.parallelism,
            rfsee_tf_idf::Runtime::available_parallelism()
        );
    }

    #[test]
    fn parallelism_can_be_overridden() {
        let args = Args::try_parse_from(["rfsee", "index", "--parallelism", "4"]).unwrap();
        assert_eq!(args.parallelism.get(), 4);
    }

    #[test]
    fn parallelism_can_precede_the_subcommand() {
        let args = Args::try_parse_from(["rfsee", "--parallelism", "2", "index"]).unwrap();
        assert_eq!(args.parallelism.get(), 2);
    }

    #[test]
    fn parallelism_rejects_zero() {
        assert!(Args::try_parse_from(["rfsee", "index", "--parallelism", "0"]).is_err());
    }

    #[test]
    fn verbosity_can_precede_the_subcommand() {
        let args = Args::try_parse_from(["rfsee", "-vv", "index"]).unwrap();
        assert_eq!(args.verbose, 2);
    }

    #[test]
    fn verbosity_can_follow_the_subcommand() {
        let args = Args::try_parse_from(["rfsee", "index", "-vvv"]).unwrap();
        assert_eq!(args.verbose, 3);
    }

    #[test]
    fn minimum_score_is_unset_without_a_cli_flag() {
        let args = Args::try_parse_from(["rfsee", "search", "--terms", "http"]).unwrap();
        let Some(Command::Search { min_score, .. }) = args.command else {
            panic!("expected search command");
        };
        assert_eq!(min_score, None);
    }

    #[test]
    fn minimum_score_uses_displayed_decimal_scale() {
        let args = Args::try_parse_from([
            "rfsee",
            "search",
            "--terms",
            "http",
            "--min-score",
            "0.123456789",
        ])
        .unwrap();
        let Some(Command::Search { min_score, .. }) = args.command else {
            panic!("expected search command");
        };
        assert_eq!(min_score, Some(123_456_789));
    }

    #[test]
    fn minimum_score_is_an_exclusive_threshold() {
        let mut results = [9, 10, 11]
            .map(|score| RfcSearchResult {
                url: String::new(),
                title: String::new(),
                score,
            })
            .into();
        filter_results_by_score(&mut results, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 11);
    }
}
