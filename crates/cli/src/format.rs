use std::time::{SystemTime, UNIX_EPOCH};

use rfsee_tf_idf::RfcSearchResult;

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

pub(crate) fn format_search_results_tsv(results: &[RfcSearchResult]) -> String {
    // TSV uses tabs between fields and one record per line; tabs and line breaks cannot occur
    // inside fields. https://www.iana.org/assignments/media-types/text/tab-separated-values
    let mut output = String::from("url\ttitle\tscore");
    for result in results {
        output.push('\n');
        output.push_str(&sanitize_tsv_field(&result.url));
        output.push('\t');
        output.push_str(&sanitize_tsv_field(&result.title));
        output.push('\t');
        output.push_str(&format_score(result.score));
    }
    output
}

fn sanitize_tsv_field(field: &str) -> String {
    field
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            character => character,
        })
        .collect()
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

pub(crate) fn parse_score(input: &str) -> Result<i32, String> {
    let (negative, unsigned) = match input.as_bytes().first() {
        Some(b'-') => (true, &input[1..]),
        Some(b'+') => (false, &input[1..]),
        _ => (false, input),
    };
    let (whole, fractional) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if (whole.is_empty() && fractional.is_empty())
        || !whole.chars().all(|character| character.is_ascii_digit())
        || !fractional
            .chars()
            .all(|character| character.is_ascii_digit())
        || fractional.len() > 9
    {
        return Err("score must be a decimal number with at most 9 fractional digits".into());
    }

    let whole = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<i64>()
            .map_err(|_| "score is outside the supported range")?
    };
    let fractional = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<i64>()
            .map_err(|_| "score is outside the supported range")?
            * 10_i64.pow(9 - fractional.len() as u32)
    };
    let magnitude = whole
        .checked_mul(STORED_SCORE_SCALE)
        .and_then(|value| value.checked_add(fractional))
        .ok_or("score is outside the supported range")?;
    let scaled = if negative { -magnitude } else { magnitude };
    i32::try_from(scaled).map_err(|_| "score is outside the supported range".into())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use rfsee_tf_idf::RfcSearchResult;

    use super::{
        format_log_line, format_score, format_search_results_tsv, format_timestamp, parse_score,
    };

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
    fn search_output_is_tsv_with_a_header() {
        let output = format_search_results_tsv(&search_results(2));

        assert_eq!(
            output,
            "url\ttitle\tscore\nhttps://example.com/1\tResult 1\t0.0000001\nhttps://example.com/2\tResult 2\t0.0000002"
        );
    }

    #[test]
    fn search_output_includes_every_result() {
        let output = format_search_results_tsv(&search_results(13));
        assert_eq!(output.lines().count(), 14);
        assert!(output.contains("https://example.com/13\tResult 13\t0.0000013"));
    }

    #[test]
    fn search_output_replaces_tsv_record_separators_inside_fields() {
        let output = format_search_results_tsv(&[RfcSearchResult {
            url: "https://example.com/a\tb".to_string(),
            title: "A title\r\nwith lines".to_string(),
            score: 0,
        }]);
        assert_eq!(
            output,
            "url\ttitle\tscore\nhttps://example.com/a b\tA title  with lines\t0"
        );
    }

    #[test]
    fn empty_search_output_is_a_header_only() {
        assert_eq!(format_search_results_tsv(&[]), "url\ttitle\tscore");
    }

    #[test]
    fn scores_are_displayed_as_unscaled_decimals() {
        assert_eq!(format_score(123_456_789), "0.123456789");
        assert_eq!(format_score(1_500_000_000), "1.5");
        assert_eq!(format_score(-1), "-0.000000001");
        assert_eq!(format_score(0), "0");
    }

    #[test]
    fn score_parser_uses_the_stored_score_scale() {
        assert_eq!(parse_score("0").unwrap(), 0);
        assert_eq!(parse_score(".5").unwrap(), 500_000_000);
        assert_eq!(parse_score("-0.000000001").unwrap(), -1);
        assert_eq!(parse_score("2.147483647").unwrap(), i32::MAX);
        assert_eq!(parse_score("-2.147483648").unwrap(), i32::MIN);
    }

    #[test]
    fn score_parser_rejects_invalid_or_unrepresentable_values() {
        for input in ["", ".", "1.0000000000", "nan", "2.147483648"] {
            assert!(parse_score(input).is_err(), "accepted {input:?}");
        }
    }
}
