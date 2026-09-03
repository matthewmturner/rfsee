//! Deterministic synthetic corpus generation shared by the criterion benches and the
//! memory profiler binary.
//!
//! The corpus mimics the real RFC corpus shape: a fixed vocabulary sampled with a
//! harmonic (Zipf-ish) distribution so a few terms appear in nearly every document and
//! most terms are rare. Generation is seeded, so every run produces identical data.

use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::SmallRng;
use rand::SeedableRng;
use rfsee_tf_idf::{RfcEntry, TfIdf};

/// Default number of synthetic documents; override with the RFSEE_BENCH_DOCS env var.
pub const DEFAULT_DOCS: usize = 1_000;
/// Tokens generated per document, roughly matching a real RFC's word count.
pub const TOKENS_PER_DOC: usize = 2_500;
/// Size of the vocabulary documents are sampled from.
pub const VOCAB_SIZE: usize = 20_000;
/// Fixed seed for reproducible corpora across runs and machines.
const SEED: u64 = 0x5EED;

/// Number of documents to use, from `RFSEE_BENCH_DOCS` or [`DEFAULT_DOCS`].
pub fn doc_count_from_env() -> usize {
    std::env::var("RFSEE_BENCH_DOCS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_DOCS)
}

fn vocabulary() -> Vec<String> {
    (0..VOCAB_SIZE).map(|i| format!("term{i}")).collect()
}

/// Weights 1, 1/2, 1/3, ... so term0 is "the"-like common and high indices are rare.
fn harmonic_weights() -> WeightedIndex<f64> {
    WeightedIndex::new((1..=VOCAB_SIZE).map(|i| 1.0 / i as f64)).unwrap()
}

fn make_entry(
    vocab: &[String],
    dist: &WeightedIndex<f64>,
    rng: &mut SmallRng,
    number: i32,
) -> RfcEntry {
    let mut content = String::with_capacity(TOKENS_PER_DOC * 8);
    for _ in 0..TOKENS_PER_DOC {
        content.push_str(&vocab[dist.sample(rng)]);
        content.push(' ');
    }
    RfcEntry {
        number,
        url: format!("https://rfsee.com/{number}"),
        title: format!("RFC {number}"),
        content: Some(content),
    }
}

/// The whole corpus materialized at once, mirroring what `par_load_rfcs` does today:
/// every document's content is held in memory before processing starts.
pub fn corpus(n_docs: usize) -> Vec<RfcEntry> {
    corpus_stream(n_docs).collect()
}

/// A streaming view of the corpus that generates one document at a time, mirroring
/// the shape of the pipeline after fetched documents are processed as they arrive.
pub fn corpus_stream(n_docs: usize) -> CorpusStream {
    CorpusStream {
        vocab: vocabulary(),
        dist: harmonic_weights(),
        rng: SmallRng::seed_from_u64(SEED),
        next: 1,
        total: n_docs as i32,
    }
}

pub struct CorpusStream {
    vocab: Vec<String>,
    dist: WeightedIndex<f64>,
    rng: SmallRng,
    next: i32,
    total: i32,
}

impl Iterator for CorpusStream {
    type Item = RfcEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next > self.total {
            return None;
        }
        let entry = make_entry(&self.vocab, &self.dist, &mut self.rng, self.next);
        self.next += 1;
        Some(entry)
    }
}

extern "C" fn noop_cb(_: *const std::ffi::c_char) {}

/// Process entries into a finished index (term frequencies + IDF + term scores).
pub fn build_index(entries: impl IntoIterator<Item = RfcEntry>) -> TfIdf {
    let mut index = TfIdf::default();
    for entry in entries {
        index.add_rfc_entry(entry);
    }
    index.finish(noop_cb);
    index
}
