use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rfsee_tf_idf::RfcSearchResult;

const MAX_DISPLAYED_SEARCH_RESULTS: usize = 10;
const STORED_SCORE_SCALE: i64 = 1_000_000_000;

/// Format a UTC timestamp using the Internet date/time format specified by
/// RFC 3339 section 5.6: https://www.rfc-editor.org/rfc/rfc3339.html#section-5.6
pub(crate) fn format_timestamp(timestamp: SystemTime) -> String {
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

pub(crate) fn format_log_line(timestamp: SystemTime, msg: impl std::fmt::Display) -> String {
    format!("[{}] {msg}", format_timestamp(timestamp))
}

pub(crate) fn format_search_results(
    results: &[RfcSearchResult],
    execution_time: Duration,
) -> String {
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

pub(crate) fn format_score(score: i32) -> String {
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use rfsee_tf_idf::RfcSearchResult;

    use super::{format_log_line, format_score, format_search_results, format_timestamp};

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
