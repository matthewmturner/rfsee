use std::{
    fs::File,
    num::NonZeroUsize,
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clap::{ArgAction, Parser, Subcommand};
use rfsee_tf_idf::{
    error::{RFSeeError, RFSeeResult},
    get_index_path, search_index, Index, RfcSearchResult, Runtime, TfIdf,
};

const MAX_DISPLAYED_SEARCH_RESULTS: usize = 10;
const STORED_SCORE_SCALE: i64 = 1_000_000_000;

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

/// Format a UTC timestamp using the Internet date/time format specified by
/// RFC 3339 section 5.6: https://www.rfc-editor.org/rfc/rfc3339.html#section-5.6
fn format_timestamp(timestamp: SystemTime) -> String {
    let since_epoch = timestamp.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since_epoch.as_secs();
    let milliseconds = since_epoch.subsec_millis();
    let seconds_today = seconds % 86_400;

    // Convert days since the Unix epoch to a Gregorian calendar date.
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = month_position + if month_position < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    let hour = seconds_today / 3_600;
    let minute = seconds_today % 3_600 / 60;
    let second = seconds_today % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milliseconds:03}Z")
}

fn format_log_line(timestamp: SystemTime, msg: impl std::fmt::Display) -> String {
    format!("[{}] {msg}", format_timestamp(timestamp))
}

fn log(level: u8, msg: impl std::fmt::Display) {
    if VERBOSITY.load(Ordering::Relaxed) >= level {
        eprintln!("{}", format_log_line(SystemTime::now(), msg));
    }
}

fn log_progress(message: &str) {
    log(2, message);
}

fn format_search_results(results: &[RfcSearchResult], execution_time: Duration) -> String {
    let displayed_count = results.len().min(MAX_DISPLAYED_SEARCH_RESULTS);
    let remaining_count = results.len() - displayed_count;
    let mut output = String::from("Docs: [");

    for result in &results[..displayed_count] {
        output.push_str(&format!(
            "\n    RfcSearchResult {{\n        url: {:?},\n        title: {:?},\n        score: {},\n    }},",
            result.url,
            result.title,
            format_score(result.score),
        ));
    }

    output.push_str(&format!(
        "]\nRemaining results: {remaining_count}\nSearch execution time: {execution_time:?}"
    ));
    output
}

fn format_score(score: i32) -> String {
    let score = i64::from(score);
    let sign = if score < 0 { "-" } else { "" };
    let magnitude = score.abs();
    let whole = magnitude / STORED_SCORE_SCALE;
    let fractional = format!("{:09}", magnitude % STORED_SCORE_SCALE);
    let fractional = fractional.trim_end_matches('0');

    if fractional.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fractional}")
    }
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
    use std::time::{Duration, SystemTime};

    use clap::Parser;

    use rfsee_tf_idf::RfcSearchResult;

    use super::{format_log_line, format_score, format_search_results, format_timestamp, Args};

    fn search_results(count: usize) -> Vec<RfcSearchResult> {
        (1..=count)
            .map(|number| RfcSearchResult {
                url: format!("https://example.com/{number}"),
                title: format!("Result {number}"),
                score: number as i32 * 100,
            })
            .collect()
    }

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
    fn timestamp_is_utc_with_millisecond_precision() {
        let timestamp = SystemTime::UNIX_EPOCH
            + Duration::from_secs(1_704_067_200)
            + Duration::from_millis(123);
        assert_eq!(format_timestamp(timestamp), "2024-01-01T00:00:00.123Z");
    }

    #[test]
    fn log_lines_include_an_rfc3339_timestamp() {
        assert_eq!(
            format_log_line(SystemTime::UNIX_EPOCH, "Loading index"),
            "[1970-01-01T00:00:00.000Z] Loading index"
        );
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
    fn search_output_shows_all_results_when_there_are_ten_or_fewer() {
        let output = format_search_results(&search_results(10), Duration::from_micros(123));

        assert!(output.contains("Result 10"));
        assert_eq!(output.matches("title: \"Result ").count(), 10);
        assert_eq!(output.matches("score: ").count(), 10);
        assert!(output.contains("score: 0.000001"));
        assert!(output.contains("Remaining results: 0"));
        assert!(output.ends_with("Search execution time: 123µs"));
    }

    #[test]
    fn search_output_shows_ten_results_and_the_remaining_count() {
        let output = format_search_results(&search_results(13), Duration::from_millis(2));

        assert!(output.contains("Result 10"));
        assert!(!output.contains("Result 11"));
        assert_eq!(output.matches("title: \"Result ").count(), 10);
        assert_eq!(output.matches("score: ").count(), 10);
        assert!(!output.contains("score: 0.0000011"));
        assert!(output.contains("Remaining results: 3"));
        assert!(output.ends_with("Search execution time: 2ms"));
    }

    #[test]
    fn scores_are_displayed_as_unscaled_decimals() {
        assert_eq!(format_score(123_456_789), "0.123456789");
        assert_eq!(format_score(1_500_000_000), "1.5");
        assert_eq!(format_score(-1), "-0.000000001");
        assert_eq!(format_score(0), "0");
    }
}
