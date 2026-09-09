use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use rfsee_tf_idf::{
    error::{RFSeeError, RFSeeResult},
    get_index_path,
};
use serde::Deserialize;

use crate::format::parse_score;

const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_MIN_SCORE: i32 = 1_000_000;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    min_score: Option<ScoreValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScoreValue {
    Integer(i64),
    Float(f64),
}

#[derive(Debug, Default)]
pub(crate) struct Config {
    pub(crate) min_score: Option<i32>,
}

impl Config {
    pub(crate) fn resolve_min_score(&self, cli_value: Option<i32>) -> i32 {
        cli_value.or(self.min_score).unwrap_or(DEFAULT_MIN_SCORE)
    }
}

pub(crate) fn load(custom_path: Option<&Path>) -> RFSeeResult<Config> {
    let path = match custom_path {
        Some(path) => path.to_owned(),
        None => default_path()?,
    };
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if custom_path.is_none() && error.kind() == ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) => {
            return Err(RFSeeError::IOError(format!(
                "Could not read config {}: {error}",
                path.display()
            )));
        }
    };
    parse(&contents).map_err(|message| {
        RFSeeError::ParseError(format!("Invalid config {}: {message}", path.display()))
    })
}

fn default_path() -> RFSeeResult<PathBuf> {
    Ok(get_index_path(None)?.with_file_name(CONFIG_FILE_NAME))
}

fn parse(contents: &str) -> Result<Config, String> {
    let file: FileConfig = toml::from_str(contents).map_err(|error| error.to_string())?;
    let min_score = file
        .min_score
        .map(|score| match score {
            ScoreValue::Integer(value) => parse_score(&value.to_string()),
            ScoreValue::Float(value) if value.is_finite() => parse_score(&value.to_string()),
            ScoreValue::Float(_) => Err("min_score must be finite".into()),
        })
        .transpose()
        .map_err(|error| format!("invalid min_score: {error}"))?;
    Ok(Config { min_score })
}

#[cfg(test)]
mod tests {
    use super::{parse, Config, DEFAULT_MIN_SCORE};

    #[test]
    fn parses_decimal_minimum_score() {
        assert_eq!(
            parse("min_score = 0.123456789").unwrap().min_score,
            Some(123_456_789)
        );
        assert_eq!(
            parse("min_score = 1").unwrap().min_score,
            Some(1_000_000_000)
        );
    }

    #[test]
    fn allows_an_empty_config() {
        assert_eq!(parse("").unwrap().min_score, None);
    }

    #[test]
    fn rejects_unknown_or_invalid_settings() {
        assert!(parse("minimum_score = 0.1").is_err());
        assert!(parse("min_score = 3").is_err());
    }

    #[test]
    fn cli_value_overrides_config_then_falls_back_to_default() {
        let configured = Config {
            min_score: Some(100),
        };
        assert_eq!(configured.resolve_min_score(None), 100);
        assert_eq!(configured.resolve_min_score(Some(200)), 200);
        assert_eq!(Config::default().resolve_min_score(None), DEFAULT_MIN_SCORE);
    }
}
