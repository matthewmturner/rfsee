use std::{
    fs::File,
    num::NonZeroUsize,
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
    time::{Instant, SystemTime},
};

use clap::{ArgAction, Parser, Subcommand};
use rfsee_tf_idf::{
    error::{RFSeeError, RFSeeResult},
    get_index_path, search_index, Index, Runtime, TfIdf,
};

mod format;

use format::{format_log_line, format_search_results};

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
        #[arg(short, long)]
        terms: String,
        #[arg(short, long)]
        index_path: Option<PathBuf>,
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

fn handle_command(args: Args, runtime: &Runtime) -> RFSeeResult<()> {
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
            Command::Search { terms, index_path } => {
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
                let results = search_index(terms, index);
                let search_execution_time = search_start.elapsed();
                log(2, format!("Search completed in {search_execution_time:?}"));
                log(2, format!("Results: {}", results.len()));
                println!("{}", format_search_results(&results, search_execution_time));
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

    use super::Args;

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
}
