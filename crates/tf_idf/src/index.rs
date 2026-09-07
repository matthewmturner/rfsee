use std::{
    collections::HashMap,
    ffi::{c_char, CString},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::available_parallelism,
    time::Duration,
};

use crate::{
    error::{RFSeeError, RFSeeResult},
    fetch::{fetch, fetch_rfc, fetch_rfc_index, RFC_EDITOR_URL_BASE},
    parse::{parse_rfc_details, parse_rfc_index},
    path::home_dir,
    threadpool,
};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// A term in a document
pub type Term = String;
/// The url of the source document
pub type Url = String;
/// Term frequency, tf(t,d), is the relative frequency of term t within document d.  Keys are the
/// terms and values are the frequency of the term
pub type TermFreqs = HashMap<Term, f32>;
/// Mapping from a Url to the term frequencies for the document at that url.
pub type DocTermFreqs = HashMap<Url, TermFreqs>;
/// For each term its inverse document frequency which is computed as:
///
/// log(total_docs / (docs_with_term + ε))
///
/// ε used so that terms which are in all documents, such as "HTTP", can still have their term
/// frequency apply.  Without this the final score would be 0 as the IDF would be 0.
pub type InvDocFreqs = HashMap<Term, f32>;
/// Map of terms to a vector of documents that have that term
pub type DocsWithTerm = HashMap<Term, Vec<Url>>;
pub type ProcessedRfcs = HashMap<Url, ProcessedRfc>;
pub type RfcNumber = i32;
pub type TermScore = i32;
pub type RfcDetailsMap = HashMap<RfcNumber, RfcDetails>;

const RFC_EDITOR_FILE_TYPE: &str = "txt";
const WORD_MATCH_REGEX: &str = r"(\w+)";
/// We have an epsilon value to account for some terms, like "HTTP", being in all RFCs.
const EPSILON: f32 = 0.0001;
/// How to split the user provided search
const SEARCH_TERMS_DELIMITER: &str = " ";
const INDEX_FILE_NAME: &str = "index.json";
const DEFAULT_INDEX_PATH: &str = "/tmp/index.json";

/// Configuration for loading RFCs into the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadConfig {
    /// Number of worker threads used to fetch RFCs concurrently.
    pub parallelism: NonZeroUsize,
}

impl LoadConfig {
    /// Fallback used when the platform cannot report its available parallelism.
    const FALLBACK_PARALLELISM: usize = 1;

    /// Build a config with the given number of worker threads.
    pub fn with_parallelism(parallelism: NonZeroUsize) -> Self {
        Self { parallelism }
    }

    /// The parallelism reported by the platform, i.e. `std::thread::available_parallelism`,
    /// falling back to a single thread when it cannot be determined.
    pub fn default_parallelism() -> NonZeroUsize {
        available_parallelism().unwrap_or_else(|_| {
            NonZeroUsize::new(Self::FALLBACK_PARALLELISM).expect("fallback is non-zero")
        })
    }
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            parallelism: Self::default_parallelism(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RfcEntry {
    pub number: i32,
    pub url: String,
    pub title: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RfcLoadFailure {
    pub rfc: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default)]
pub struct RfcLoadReport {
    pub total: usize,
    pub loaded: Vec<String>,
    pub failures: Vec<RfcLoadFailure>,
}

pub struct ProcessedRfc {
    number: i32,
    term_freqs: TermFreqs,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RfcDetails {
    title: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Index {
    pub rfc_details: RfcDetailsMap,
    /// Map of terms to map of RFC numbers with that term and the terms score in that RFC
    pub term_scores: HashMap<Term, HashMap<RfcNumber, TermScore>>,
}

#[repr(C)]
#[derive(Debug)]
pub struct RfcSearchResult {
    pub url: String,
    pub title: String,
}

pub fn get_index_path(custom_path: Option<PathBuf>) -> RFSeeResult<PathBuf> {
    if let Some(path) = custom_path {
        Ok(path)
    } else if let Some(home_dir) = home_dir() {
        let rfsee_config_dir = home_dir.join(".config/rfsee");
        if !rfsee_config_dir.exists() {
            std::fs::create_dir_all(&rfsee_config_dir).unwrap()
        }
        Ok(rfsee_config_dir.join(INDEX_FILE_NAME))
    } else if cfg!(unix) {
        Ok(PathBuf::from(DEFAULT_INDEX_PATH))
    } else {
        Err(RFSeeError::IOError(
            "No default location for index on Windows".to_string(),
        ))
    }
}

#[repr(C)]
#[derive(Default)]
pub struct TfIdf {
    /// Term frequencies for the document at a url.
    pub doc_tfs: DocTermFreqs,
    /// The inverse document frequency is a measure of how much information the word provides, i.e.,
    /// how common or rare it is across all documents. It is the logarithmically scaled inverse fraction
    /// of the documents that contain the word (obtained by dividing the total number of documents by
    /// the number of documents containing the term, and then taking the logarithm of that quotient).
    pub idfs: InvDocFreqs,
    /// Map of terms to a list of documents that contain that term
    pub docs_with_term: DocsWithTerm,
    pub processed_rfcs: ProcessedRfcs,
    pub index: Index,
}

impl TfIdf {
    pub fn load_rfcs(&mut self) -> RFSeeResult<()> {
        let raw_rfc_index = fetch_rfc_index()?;
        let raw_rfcs = parse_rfc_index(&raw_rfc_index)?;
        for raw_rfc in raw_rfcs {
            let (rfc_num, title) = parse_rfc_details(raw_rfc)?;
            let url = format!("{RFC_EDITOR_URL_BASE}{rfc_num}.txt");
            let maybe_parsed_rfc = match fetch(&url) {
                Ok(content) => Some(RfcEntry {
                    number: rfc_num,
                    url: url.clone(),
                    title: title.replace("\n     ", " ").to_string(),
                    content: Some(content),
                }),
                Err(_) => None,
            };
            if let Some(parsed_rfc) = maybe_parsed_rfc {
                if parsed_rfc.content.is_some() {
                    self.add_rfc_entry(parsed_rfc)
                }
            }
        }

        Ok(())
    }

    /// Load the RFCs in parallel using a threadpool sized from `LoadConfig::default`.
    pub fn par_load_rfcs(
        &mut self,
        progress_cb: extern "C" fn(progress: *const c_char),
    ) -> RFSeeResult<()> {
        self.par_load_rfcs_with_report(progress_cb).map(|_| ())
    }

    /// Load the RFCs in parallel using a threadpool sized from `LoadConfig::default` and return
    /// details about successful and failed fetches.
    pub fn par_load_rfcs_with_report(
        &mut self,
        progress_cb: extern "C" fn(progress: *const c_char),
    ) -> RFSeeResult<RfcLoadReport> {
        self.par_load_rfcs_with_config(LoadConfig::default(), progress_cb)
    }

    /// Load the RFCs in parallel using the provided `LoadConfig` and return details about
    /// successful and failed fetches.
    pub fn par_load_rfcs_with_config(
        &mut self,
        config: LoadConfig,
        progress_cb: extern "C" fn(progress: *const c_char),
    ) -> RFSeeResult<RfcLoadReport> {
        let pool = threadpool::ThreadPool::new(config.parallelism.get());
        let raw_rfc_index = fetch_rfc_index()?;
        let raw_rfcs: Vec<_> = parse_rfc_index(&raw_rfc_index)?
            .into_iter()
            .filter(|rfc| !rfc.trim().is_empty())
            .collect();
        let rfcs_count = raw_rfcs.len();

        let parsed_rfcs: Vec<RfcEntry> = Vec::new();
        let parsed_rfcs = Arc::new(Mutex::new(parsed_rfcs));
        let failures: Vec<RfcLoadFailure> = Vec::new();
        let failures = Arc::new(Mutex::new(failures));

        let remaining = raw_rfcs.len();
        let remaining = Arc::new(Mutex::new(remaining));

        for rfc in raw_rfcs.into_iter() {
            let string = rfc.to_string();
            let remaining = Arc::clone(&remaining);
            let parsed_rfcs = Arc::clone(&parsed_rfcs);
            let failures = Arc::clone(&failures);
            pool.execute(move || {
                match fetch_rfc(&string) {
                    Ok(rfc) => parsed_rfcs.lock().unwrap().push(rfc),
                    Err(err) => failures.lock().unwrap().push(RfcLoadFailure {
                        rfc: string
                            .split_whitespace()
                            .next()
                            .unwrap_or("unknown")
                            .to_string(),
                        reason: err.to_string().trim().to_string(),
                    }),
                }
                let mut guard = remaining.lock().unwrap();
                *guard -= 1;
            })?
        }

        let mut finished = false;
        while !finished {
            let remaining = remaining.clone();
            let guard = remaining.lock().unwrap();
            // Need to log here, and not in the thread pool because we cant have different threads
            // call the callback
            if let Ok(msg) = CString::new(format!("{} remaining RFCs to fetch", *guard)) {
                progress_cb(msg.as_ptr())
            }
            if *guard == 0 {
                finished = true
            } else {
                drop(guard);
                // Don't want to go crazy locking the Mutex, so we only check every 5 seconds
                std::thread::sleep(Duration::from_secs(5));
            }
        }

        let mut loaded = Vec::new();
        match Arc::try_unwrap(parsed_rfcs) {
            Ok(mutex) => match mutex.into_inner() {
                Ok(rfcs) => {
                    for (i, rfc) in rfcs.into_iter().enumerate() {
                        loaded.push(rfc.url.clone());
                        self.add_rfc_entry(rfc);
                        if i % 100 == 0 {
                            let progress = (i as f64 / rfcs_count as f64) * 100_f64;
                            if let Ok(msg) =
                                CString::new(format!("Parse progress: {progress:0.0}%"))
                            {
                                progress_cb(msg.as_ptr())
                            }
                        }
                    }
                }
                Err(err) => return Err(RFSeeError::RuntimeError(err.to_string())),
            },
            Err(_) => {
                return Err(RFSeeError::RuntimeError(
                    "More than one reference remaining".to_string(),
                ))
            }
        }

        let mut failures = Arc::try_unwrap(failures)
            .map_err(|_| {
                RFSeeError::RuntimeError("More than one failure reference remaining".to_string())
            })?
            .into_inner()
            .map_err(|err| RFSeeError::RuntimeError(err.to_string()))?;
        loaded.sort();
        failures.sort_by(|a, b| a.rfc.cmp(&b.rfc));

        Ok(RfcLoadReport {
            total: rfcs_count,
            loaded,
            failures,
        })
    }

    /// Process `RfcEntry` by computing it's term frequencies and add it to index
    pub fn add_rfc_entry(&mut self, rfc: RfcEntry) {
        let re = Regex::new(WORD_MATCH_REGEX).unwrap();

        if let Some(content) = &rfc.content {
            let mut term_counts: HashMap<&str, usize> = HashMap::new();
            let mut tfs = TermFreqs::new();
            let mut terms = 0;

            for found in re.find_iter(content) {
                if let Some(k) = term_counts.get_mut(found.as_str()) {
                    *k += 1
                } else {
                    term_counts.insert(found.as_str(), 1);
                }
                terms += 1
            }

            for (t, c) in term_counts {
                let frequency = c as f32 / terms as f32;
                tfs.insert(t.to_string(), frequency);
            }

            let indexed_rfc = ProcessedRfc {
                number: rfc.number,
                term_freqs: tfs,
            };

            self.index
                .rfc_details
                .insert(rfc.number, RfcDetails { title: rfc.title });

            self.processed_rfcs.insert(rfc.url, indexed_rfc);
        }
    }

    /// Take all the processed documents and their term frequencies to compute the final term
    /// scores
    pub fn finish(&mut self, progress_cb: extern "C" fn(*const c_char)) {
        if let Ok(msg) = CString::new("Collecting terms") {
            progress_cb(msg.as_ptr())
        }
        // First, we collect all terms and the number of docs they appear in
        let mut term_counts: HashMap<&String, usize> = HashMap::new();
        for indexed_rfc in self.processed_rfcs.values() {
            for term in indexed_rfc.term_freqs.keys() {
                if let Some(v) = term_counts.get(term) {
                    term_counts.insert(term, v + 1);
                } else {
                    term_counts.insert(term, 1);
                }
            }
        }

        if let Ok(msg) = CString::new("Computing inverse document frequencies") {
            progress_cb(msg.as_ptr())
        }
        // Then we compute the inverse document frequency for each term
        let total_docs = self.processed_rfcs.len();
        for (term, docs_with_term) in term_counts {
            let inv_fraction = (total_docs as f32) / ((docs_with_term as f32) + EPSILON);
            let scaled = inv_fraction.log10();
            self.idfs.insert(term.clone(), scaled);
        }

        if let Ok(msg) = CString::new("Scoring documents") {
            progress_cb(msg.as_ptr())
        }
        // Then we compute the score for each term in all documents
        self.processed_rfcs.iter().for_each(|(_doc, rfc)| {
            for (doc_term, freq) in &rfc.term_freqs {
                if let Some(idf) = self.idfs.get(doc_term) {
                    // there are often lots of 0s preceding actual score, we can remove those and
                    // convert to integer to save some space
                    let doc_term_score = (freq * idf) * 1_000_000_000.0;
                    let rounded_doc_term_score = doc_term_score.round() as i32;
                    if let Some(term_scores_per_doc) = self.index.term_scores.get_mut(doc_term) {
                        term_scores_per_doc.insert(rfc.number, rounded_doc_term_score);
                    } else {
                        let mut term_scores_per_doc = HashMap::new();
                        term_scores_per_doc.insert(rfc.number, rounded_doc_term_score);
                        self.index
                            .term_scores
                            .insert(doc_term.to_string(), term_scores_per_doc);
                    }
                }
            }
        });
    }

    /// Save index to disk
    pub fn save(&self, path: &Path) {
        {
            let index_file = std::fs::File::create(path).unwrap();
            simd_json::to_writer(index_file, &self.index).unwrap();
        }
    }
}

/// Combine the result set for each term into a single result set where a document only shows up
/// once.  Scores are combined by adding them.
pub fn combine_scores(scores: Vec<HashMap<i32, i32>>) -> Vec<i32> {
    let mut combined_scores: HashMap<i32, i32> = HashMap::new();
    for score in scores {
        for (rfc_num, term_score) in score {
            if let Some(combined_doc_score) = combined_scores.get_mut(&rfc_num) {
                *combined_doc_score += term_score;
            } else {
                combined_scores.insert(rfc_num, term_score);
            }
        }
    }

    let mut scores_list: Vec<(i32, i32)> = combined_scores.into_iter().collect();

    // Sort by score in descending order
    scores_list.sort_by(|(_, a_score), (_, b_score)| b_score.partial_cmp(a_score).unwrap());

    // Return only the URLs
    scores_list.into_iter().map(|(rfc, _)| rfc).collect()
}

/// Search the provided index for the terms and return ordered results
pub fn search_index(search: String, index: Index) -> Vec<RfcSearchResult> {
    // Extract all the terms from the search
    let terms: Vec<&str> = search.split(SEARCH_TERMS_DELIMITER).collect();

    // Extract the top documents for each term
    let mut scores = Vec::new();
    for term in terms {
        if let Some(term_scores) = index.term_scores.get(term) {
            scores.push(term_scores.clone());
        }
    }

    // Combine the scores by adding them for each document
    let rfcs = combine_scores(scores);
    rfcs.iter()
        .map(|n| {
            if let Some(details) = index.rfc_details.get(n) {
                RfcSearchResult {
                    url: format!("{RFC_EDITOR_URL_BASE}{n}.{RFC_EDITOR_FILE_TYPE}"),
                    title: details.title.clone(),
                }
            } else {
                RfcSearchResult {
                    url: format!("{RFC_EDITOR_URL_BASE}{n}.{RFC_EDITOR_FILE_TYPE}"),
                    title: "MISSING TITLE".to_string(),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_char, num::NonZeroUsize};

    use super::{parse_rfc_index, LoadConfig, RfcEntry, TfIdf};

    extern "C" fn dummy_cb(_msg: *const c_char) {}

    #[test]
    fn test_load_config_defaults_to_available_parallelism() {
        let config = LoadConfig::default();
        let expected =
            std::thread::available_parallelism().unwrap_or_else(|_| NonZeroUsize::new(1).unwrap());
        assert_eq!(config.parallelism, expected);
    }

    #[test]
    fn test_load_config_with_parallelism() {
        let config = LoadConfig::with_parallelism(NonZeroUsize::new(3).unwrap());
        assert_eq!(config.parallelism.get(), 3);
    }

    #[test]
    fn test_parse_index() {
        let index_contents = std::fs::read_to_string("../../data/rfc_index.txt").unwrap();
        let parsed = parse_rfc_index(&index_contents).unwrap();

        assert_eq!(parsed, vec!["0001 Host Software. S. Crocker. April 1969. (Format: TXT, HTML) (Status:\n     UNKNOWN) (DOI: 10.17487/RFC0001) ", "0002 Host software. B. Duvall. April 1969. (Format: TXT, PDF, HTML)\n     (Status: UNKNOWN) (DOI: 10.17487/RFC0002) ", ""]);
    }

    #[test]
    fn test_index_single_file() {
        let mut tf_idf = TfIdf::default();
        let entry = RfcEntry {
            content: Some("Hello world!".to_string()),
            title: "Test".to_string(),
            number: 1,
            url: "https://www.rfsee.com/1".to_string(),
        };
        tf_idf.add_rfc_entry(entry);
        tf_idf.finish(dummy_cb);

        assert_eq!(tf_idf.index.rfc_details.len(), 1);
        assert_eq!(tf_idf.index.term_scores.len(), 2);

        let hello = tf_idf.index.term_scores.get("Hello");
        assert!(hello.is_some());
        let hello_doc_score = hello.unwrap().get(&1);
        assert!(hello_doc_score.is_some());
    }

    #[test]
    fn test_index_single_file_dupe_words() {
        let mut tf_idf = TfIdf::default();
        let entry = RfcEntry {
            content: Some("Hello hello!".to_string()),
            title: "Test".to_string(),
            number: 1,
            url: "https://www.rfsee.com/1".to_string(),
        };
        tf_idf.add_rfc_entry(entry);
        tf_idf.finish(dummy_cb);

        assert_eq!(tf_idf.index.rfc_details.len(), 1);
        // This should be 1 once we update parsing to treat "Hello" and "hello" the same
        assert_eq!(tf_idf.index.term_scores.len(), 2);

        let hello = tf_idf.index.term_scores.get("Hello");
        assert!(hello.is_some());
    }
}
